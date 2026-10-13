//! Synchronous deterministic event delivery. EventBus is intentionally mutably
//! owned by the host event loop. Callbacks must be short; dispatch backend work
//! separately. Before readiness, each window has its own bounded FIFO queue.
//! With panic unwinding, a failed callback does not prevent remaining listeners,
//! queued events, or broadcast destinations from receiving their deliveries.
//! The operation returns `CoreError::BackendPanic` after completing delivery;
//! callers must not retry the event merely because that error was returned.
//! A once listener is consumed before invocation, including when it panics.
use crate::{CoreError, Result, Schema, WindowId, schema::validate_name};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_BUS_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub sequence: u64,
    pub name: String,
    pub payload: Value,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ListenerId {
    owner: u64,
    sequence: u64,
}
type Listener = Box<dyn FnMut(&Event) + Send>;
struct Subscription {
    window: WindowId,
    name: String,
    once: bool,
    callback: Listener,
}
#[derive(Default)]
struct WindowEvents {
    ready: bool,
    queue: VecDeque<Event>,
}

pub struct EventBus {
    capacity: usize,
    max_payload_bytes: usize,
    windows: BTreeMap<WindowId, WindowEvents>,
    subscriptions: BTreeMap<ListenerId, Subscription>,
    schemas: BTreeMap<String, Schema>,
    sequence: u64,
    owner_id: u64,
    listener_id: u64,
}

impl EventBus {
    pub fn new(queue_capacity: usize) -> Result<Self> {
        if queue_capacity == 0 {
            return Err(CoreError::InvalidConfiguration(
                "event capacity must be positive".into(),
            ));
        }
        Ok(Self {
            capacity: queue_capacity,
            max_payload_bytes: 1024 * 1024,
            windows: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            schemas: BTreeMap::new(),
            sequence: 0,
            owner_id: allocate_bus_id()?,
            listener_id: 0,
        })
    }
    pub fn register_event(&mut self, name: impl Into<String>, schema: Schema) -> Result<()> {
        let name = name.into();
        validate_name(&name)?;
        if self.schemas.contains_key(&name) {
            return Err(CoreError::DuplicateRegistration(name));
        }
        self.schemas.insert(name, schema);
        Ok(())
    }
    pub fn open_window(&mut self, window: WindowId) -> Result<()> {
        if self.windows.contains_key(&window) {
            return Err(CoreError::DuplicateRegistration(format!(
                "window {}",
                window.0
            )));
        }
        self.windows.insert(window, WindowEvents::default());
        Ok(())
    }
    pub fn close_window(&mut self, window: WindowId) -> Result<()> {
        self.windows
            .remove(&window)
            .ok_or(CoreError::ClosedWindow)?;
        self.subscriptions.retain(|_, s| s.window != window);
        Ok(())
    }
    /// Navigation removes old page listeners and queued events, then requires a
    /// fresh readiness signal. Events emitted after navigation queue for the new page.
    pub fn navigate(&mut self, window: WindowId) -> Result<()> {
        let state = self
            .windows
            .get_mut(&window)
            .ok_or(CoreError::ClosedWindow)?;
        state.ready = false;
        state.queue.clear();
        self.subscriptions.retain(|_, s| s.window != window);
        Ok(())
    }
    pub fn subscribe(
        &mut self,
        window: WindowId,
        event: &str,
        once: bool,
        listener: impl FnMut(&Event) + Send + 'static,
    ) -> Result<ListenerId> {
        if !self.windows.contains_key(&window) {
            return Err(CoreError::ClosedWindow);
        }
        if !self.schemas.contains_key(event) {
            return Err(CoreError::InvalidConfiguration(format!(
                "unregistered event: {event}"
            )));
        }
        self.listener_id = self
            .listener_id
            .checked_add(1)
            .ok_or_else(|| CoreError::Backend("listener sequence exhausted".into()))?;
        let id = ListenerId {
            owner: self.owner_id,
            sequence: self.listener_id,
        };
        self.subscriptions.insert(
            id,
            Subscription {
                window,
                name: event.into(),
                once,
                callback: Box::new(listener),
            },
        );
        Ok(id)
    }
    /// Removes a listener belonging to this bus. A foreign or already-consumed
    /// token returns false without changing any local subscription.
    pub fn unsubscribe(&mut self, listener: ListenerId) -> bool {
        self.subscriptions.remove(&listener).is_some()
    }
    pub fn mark_ready(&mut self, window: WindowId) -> Result<()> {
        let state = self
            .windows
            .get_mut(&window)
            .ok_or(CoreError::ClosedWindow)?;
        if state.ready {
            return Ok(());
        }
        state.ready = true;
        let queued = std::mem::take(&mut state.queue);
        let mut panicked = false;
        for event in queued {
            panicked |= self.deliver(window, &event);
        }
        delivery_result(panicked)
    }
    pub fn emit<T: Serialize>(&mut self, window: WindowId, name: &str, payload: T) -> Result<()> {
        self.emit_to(&[window], name, payload)
    }
    /// Broadcast validates all queues before delivery, so overflow never produces
    /// a partially delivered broadcast. Closed windows are excluded. Callback
    /// panics are reported after every destination has been delivered or queued.
    pub fn broadcast<T: Serialize>(&mut self, name: &str, payload: T) -> Result<usize> {
        let windows: Vec<_> = self.windows.keys().copied().collect();
        self.emit_to(&windows, name, payload)?;
        Ok(windows.len())
    }
    fn emit_to<T: Serialize>(
        &mut self,
        windows: &[WindowId],
        name: &str,
        payload: T,
    ) -> Result<()> {
        let schema = self.schemas.get(name).ok_or_else(|| {
            CoreError::InvalidConfiguration(format!("unregistered event: {name}"))
        })?;
        let payload = serde_json::to_value(payload)
            .map_err(|e| CoreError::InvalidArguments(e.to_string()))?;
        schema
            .validate(&payload)
            .map_err(CoreError::InvalidArguments)?;
        if serde_json::to_vec(&payload)
            .map_err(|e| CoreError::InvalidArguments(e.to_string()))?
            .len()
            > self.max_payload_bytes
        {
            return Err(CoreError::PayloadTooLarge);
        }
        for window in windows {
            let state = self.windows.get(window).ok_or(CoreError::ClosedWindow)?;
            if !state.ready && state.queue.len() == self.capacity {
                return Err(CoreError::QueueFull);
            }
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| CoreError::Backend("event sequence exhausted".into()))?;
        let event = Event {
            sequence: self.sequence,
            name: name.into(),
            payload,
        };
        let mut panicked = false;
        for window in windows {
            let state = self.windows.get_mut(window).unwrap();
            if state.ready {
                panicked |= self.deliver(*window, &event);
            } else {
                state.queue.push_back(event.clone());
            }
        }
        delivery_result(panicked)
    }
    /// Delivers to all matching subscribers and reports whether any panicked.
    fn deliver(&mut self, window: WindowId, event: &Event) -> bool {
        let ids: Vec<_> = self
            .subscriptions
            .iter()
            .filter_map(|(id, s)| (s.window == window && s.name == event.name).then_some(*id))
            .collect();
        let mut panicked = false;
        for id in ids {
            if self.subscriptions[&id].once {
                let mut subscription = self.subscriptions.remove(&id).unwrap();
                panicked |=
                    catch_unwind(AssertUnwindSafe(|| (subscription.callback)(event))).is_err();
            } else if let Some(subscription) = self.subscriptions.get_mut(&id) {
                panicked |=
                    catch_unwind(AssertUnwindSafe(|| (subscription.callback)(event))).is_err();
            }
        }
        panicked
    }
}

fn allocate_bus_id() -> Result<u64> {
    let mut current = NEXT_BUS_ID.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .ok_or_else(|| CoreError::Backend("event bus identifier space exhausted".into()))?;
        match NEXT_BUS_ID.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return Ok(current),
            Err(actual) => current = actual,
        }
    }
}

fn delivery_result(panicked: bool) -> Result<()> {
    if panicked {
        Err(CoreError::BackendPanic)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    fn setup(capacity: usize) -> EventBus {
        let mut bus = EventBus::new(capacity).unwrap();
        bus.register_event("changed", Schema::Integer).unwrap();
        bus.open_window(WindowId(1)).unwrap();
        bus.open_window(WindowId(2)).unwrap();
        bus
    }
    fn listen(
        bus: &mut EventBus,
        window: u64,
        once: bool,
        output: Arc<Mutex<Vec<(u64, u64, Value)>>>,
    ) -> ListenerId {
        bus.subscribe(WindowId(window), "changed", once, move |event| {
            output
                .lock()
                .unwrap()
                .push((window, event.sequence, event.payload.clone()))
        })
        .unwrap()
    }
    #[test]
    fn targeted_broadcast_ready_once_unsubscribe() {
        let mut bus = setup(8);
        let output = Arc::new(Mutex::new(vec![]));
        listen(&mut bus, 1, false, output.clone());
        listen(&mut bus, 2, true, output.clone());
        let removed = listen(&mut bus, 1, false, output.clone());
        assert!(bus.unsubscribe(removed));
        assert!(!bus.unsubscribe(removed));
        bus.emit(WindowId(1), "changed", 1).unwrap();
        bus.broadcast("changed", 2).unwrap();
        assert!(output.lock().unwrap().is_empty());
        bus.mark_ready(WindowId(1)).unwrap();
        bus.mark_ready(WindowId(2)).unwrap();
        bus.broadcast("changed", 3).unwrap();
        assert_eq!(
            *output.lock().unwrap(),
            vec![
                (1, 1, 1.into()),
                (1, 2, 2.into()),
                (2, 2, 2.into()),
                (1, 3, 3.into())
            ]
        );
    }
    #[test]
    fn overflow_is_bounded_and_broadcast_atomic() {
        let mut bus = setup(1);
        let output = Arc::new(Mutex::new(vec![]));
        listen(&mut bus, 1, false, output.clone());
        bus.mark_ready(WindowId(1)).unwrap();
        bus.emit(WindowId(2), "changed", 0).unwrap();
        assert_eq!(bus.broadcast("changed", 1), Err(CoreError::QueueFull));
        assert!(output.lock().unwrap().is_empty());
    }
    #[test]
    fn navigation_removes_listeners_and_queued_data_and_close_fails() {
        let mut bus = setup(2);
        let output = Arc::new(Mutex::new(vec![]));
        listen(&mut bus, 1, false, output.clone());
        bus.emit(WindowId(1), "changed", 0).unwrap();
        bus.navigate(WindowId(1)).unwrap();
        bus.mark_ready(WindowId(1)).unwrap();
        bus.emit(WindowId(1), "changed", 1).unwrap();
        assert!(output.lock().unwrap().is_empty());
        bus.close_window(WindowId(1)).unwrap();
        assert_eq!(
            bus.emit(WindowId(1), "changed", 1),
            Err(CoreError::ClosedWindow)
        );
        assert_eq!(bus.broadcast("changed", 2), Ok(1));
    }
    #[test]
    fn event_schema_is_checked() {
        let mut bus = setup(1);
        assert!(matches!(
            bus.emit(WindowId(1), "changed", "wrong"),
            Err(CoreError::InvalidArguments(_))
        ));
        assert!(
            bus.subscribe(WindowId(9), "changed", false, |_| {})
                .is_err()
        );
    }

    #[test]
    fn foreign_bus_listener_id_cannot_unsubscribe_local_listener() {
        let mut first = setup(2);
        let mut second = setup(2);
        let output = Arc::new(Mutex::new(vec![]));
        let foreign = listen(&mut first, 1, false, output.clone());
        let local = listen(&mut second, 1, false, output.clone());
        assert_ne!(foreign, local);
        assert!(!second.unsubscribe(foreign));
        second.mark_ready(WindowId(1)).unwrap();
        second.emit(WindowId(1), "changed", 7).unwrap();
        assert_eq!(*output.lock().unwrap(), vec![(1, 1, 7.into())]);
        assert!(second.unsubscribe(local));
        assert!(first.unsubscribe(foreign));
    }

    #[test]
    fn callback_panic_does_not_starve_later_subscribers_or_replay_once() {
        let mut bus = setup(2);
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let copy = attempts.clone();
        let once = bus
            .subscribe(WindowId(1), "changed", true, move |_| {
                copy.fetch_add(1, Ordering::Relaxed);
                panic!("once subscriber failed");
            })
            .unwrap();
        let output = Arc::new(Mutex::new(vec![]));
        listen(&mut bus, 1, false, output.clone());
        bus.mark_ready(WindowId(1)).unwrap();
        assert_eq!(
            bus.emit(WindowId(1), "changed", 1),
            Err(CoreError::BackendPanic)
        );
        assert!(!bus.unsubscribe(once));
        bus.emit(WindowId(1), "changed", 2).unwrap();
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(
            *output.lock().unwrap(),
            vec![(1, 1, 1.into()), (1, 2, 2.into())]
        );
    }

    #[test]
    fn callback_panics_do_not_discard_remaining_queued_events() {
        let mut bus = setup(3);
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let copy = attempts.clone();
        let failing = bus
            .subscribe(WindowId(1), "changed", false, move |_| {
                copy.fetch_add(1, Ordering::Relaxed);
                panic!("persistent subscriber failed");
            })
            .unwrap();
        let output = Arc::new(Mutex::new(vec![]));
        listen(&mut bus, 1, false, output.clone());
        for number in 1..=3 {
            bus.emit(WindowId(1), "changed", number).unwrap();
        }
        assert_eq!(bus.mark_ready(WindowId(1)), Err(CoreError::BackendPanic));
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        assert_eq!(
            *output.lock().unwrap(),
            vec![(1, 1, 1.into()), (1, 2, 2.into()), (1, 3, 3.into())]
        );
        bus.mark_ready(WindowId(1)).unwrap();
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        assert!(bus.unsubscribe(failing));
        bus.emit(WindowId(1), "changed", 4).unwrap();
        assert_eq!(output.lock().unwrap().last(), Some(&(1, 4, 4.into())));
    }

    #[test]
    fn callback_panic_does_not_starve_ready_or_unready_broadcast_destinations() {
        let mut bus = setup(2);
        bus.open_window(WindowId(3)).unwrap();
        bus.subscribe(WindowId(1), "changed", false, |_| {
            panic!("first window failed")
        })
        .unwrap();
        let output = Arc::new(Mutex::new(vec![]));
        listen(&mut bus, 1, false, output.clone());
        listen(&mut bus, 2, false, output.clone());
        listen(&mut bus, 3, true, output.clone());
        bus.mark_ready(WindowId(1)).unwrap();
        bus.mark_ready(WindowId(2)).unwrap();
        assert_eq!(bus.broadcast("changed", 7), Err(CoreError::BackendPanic));
        assert_eq!(
            *output.lock().unwrap(),
            vec![(1, 1, 7.into()), (2, 1, 7.into())]
        );
        bus.mark_ready(WindowId(3)).unwrap();
        assert_eq!(
            *output.lock().unwrap(),
            vec![(1, 1, 7.into()), (2, 1, 7.into()), (3, 1, 7.into())]
        );
    }
}
