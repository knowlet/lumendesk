//! Display-free host bookkeeping for pull-driven stream delivery.
use crate::core::{
    Authorization, Bridge, BridgeStreamReceiver, CoreError, SessionToken, StreamResponseItem,
    WindowId,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) const MAX_STREAMS: usize = 64;
const MAX_WINDOW_STREAMS: usize = 16;
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// A cancelled but noncooperative backend still holds its permit until it exits.
#[derive(Clone, Default)]
pub(crate) struct WorkBudget(Arc<AtomicUsize>);
pub(crate) struct WorkPermit(Arc<AtomicUsize>);
impl WorkBudget {
    pub fn acquire(&self, limit: usize) -> Result<WorkPermit, CoreError> {
        let mut count = self.0.load(Ordering::Acquire);
        loop {
            if count >= limit {
                return Err(CoreError::QueueFull);
            }
            match self.0.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(WorkPermit(self.0.clone())),
                Err(actual) => count = actual,
            }
        }
    }
}
impl Drop for WorkPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Slot {
    auth: Authorization,
    receiver: Option<BridgeStreamReceiver>,
    error: Option<CoreError>,
    credit: bool,
    last_activity: Instant,
}
#[derive(Default)]
pub(crate) struct StreamBook {
    slots: BTreeMap<(WindowId, u64), Slot>,
}
impl StreamBook {
    pub fn reserve(&mut self, auth: Authorization, id: u64) -> Result<(), CoreError> {
        if self.slots.len() >= MAX_STREAMS
            || self
                .slots
                .keys()
                .filter(|(window, _)| *window == auth.window)
                .count()
                >= MAX_WINDOW_STREAMS
        {
            return Err(CoreError::QueueFull);
        }
        if self.slots.contains_key(&(auth.window, id)) {
            return Err(CoreError::ReusedRequestId);
        }
        self.slots.insert(
            (auth.window, id),
            Slot {
                auth,
                receiver: None,
                error: None,
                credit: false,
                last_activity: Instant::now(),
            },
        );
        Ok(())
    }
    pub fn attach(
        &mut self,
        auth: &Authorization,
        id: u64,
        result: Result<BridgeStreamReceiver, CoreError>,
    ) {
        if let Some(slot) = self.slots.get_mut(&(auth.window, id))
            && slot.auth.session == auth.session
        {
            match result {
                Ok(receiver) => slot.receiver = Some(receiver),
                Err(error) => slot.error = Some(error),
            }
        }
        // Dropping a late receiver cancels its producer.
    }
    pub fn credit(&mut self, auth: &Authorization, id: u64) -> Result<(), CoreError> {
        let slot = self
            .slots
            .get_mut(&(auth.window, id))
            .ok_or(CoreError::Cancelled)?;
        if slot.auth.session != auth.session {
            return Err(CoreError::StaleSession);
        }
        if slot.credit {
            return Err(CoreError::InvalidArguments(
                "only one pending stream read is supported".into(),
            ));
        }
        slot.credit = true;
        slot.last_activity = Instant::now();
        Ok(())
    }
    pub fn remove(&mut self, window: WindowId, id: u64) {
        self.slots.remove(&(window, id));
    }
    pub fn clear(&mut self, window: WindowId) {
        self.slots.retain(|(id, _), _| *id != window);
    }
    pub fn poll(&mut self, bridge: &Bridge, now: Instant) -> Vec<(WindowId, SessionToken, Value)> {
        let mut output = vec![];
        let mut remove = vec![];
        for (&(window, id), slot) in &mut self.slots {
            let cancelled = match bridge.is_request_cancelled(&slot.auth, id) {
                Ok(cancelled) => cancelled,
                Err(_) => {
                    remove.push((window, id));
                    continue;
                }
            };
            if !cancelled && now.saturating_duration_since(slot.last_activity) >= IDLE_TIMEOUT {
                let _ = bridge.cancel(&slot.auth, id);
                let result: Result<Value, CoreError> = Err(CoreError::Backend(
                    "stream exceeded the host's 60-second idle/read timeout".into(),
                ));
                output.push((
                    window,
                    slot.auth.session.clone(),
                    serde_json::json!({"id":id,"done":true,"result":result}),
                ));
                remove.push((window, id));
                continue;
            }
            // Cancellation is signalled immediately by IPC and must not depend
            // on admitting its queued command, attaching a receiver or pulling
            // another item. This check does not consume read credit or data.
            let next = if cancelled {
                Some(Err(CoreError::Cancelled))
            } else if let Some(error) = slot.error.take() {
                Some(Err(error))
            } else if slot.credit {
                slot.receiver
                    .as_ref()
                    .and_then(|receiver| match receiver.try_next() {
                        Ok(Some(item)) => Some(Ok(item)),
                        Ok(None) => None,
                        Err(error) => Some(Err(error)),
                    })
            } else {
                None
            };
            let Some(next) = next else {
                continue;
            };
            let frame = match next {
                Ok(StreamResponseItem::Value { sequence: _, value }) => {
                    slot.credit = false;
                    slot.last_activity = now;
                    serde_json::json!({"id":id,"item":value})
                }
                Ok(StreamResponseItem::Complete(result)) => {
                    remove.push((window, id));
                    serde_json::json!({"id":id,"done":true,"result":result})
                }
                Err(error) => {
                    remove.push((window, id));
                    serde_json::json!({"id":id,"done":true,"result":Result::<Value,CoreError>::Err(error)})
                }
            };
            output.push((window, slot.auth.session.clone(), frame));
        }
        for key in remove {
            self.slots.remove(&key);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_budget_remains_bounded_until_worker_releases() {
        let budget = WorkBudget::default();
        let one = budget.acquire(2).unwrap();
        let two = budget.acquire(2).unwrap();
        assert!(matches!(budget.acquire(2), Err(CoreError::QueueFull)));
        drop(one);
        assert!(budget.acquire(2).is_ok());
        drop(two);
    }
    fn fixture() -> (Bridge, Authorization) {
        use crate::core::{BridgeConfig, CallRequest, Schema, StreamMethodSchema};
        let bridge = Bridge::new(BridgeConfig::production("lumendesk://app").unwrap()).unwrap();
        bridge
            .register_stream::<u32, u32, u32, _>(
                StreamMethodSchema::new("count", Schema::Integer, Schema::Integer, Schema::Integer),
                |context, count, sink| {
                    for value in 0..count {
                        context.check_cancelled()?;
                        sink.send_wait(value)?;
                    }
                    Ok(count)
                },
            )
            .unwrap();
        let token = bridge
            .create_session(WindowId(1), "lumendesk://app/")
            .unwrap();
        let auth = Authorization {
            window: WindowId(1),
            session: token,
            page_url: "lumendesk://app/".into(),
            top_level: true,
        };
        let _ = CallRequest {
            id: 1,
            method: "count".into(),
            args: serde_json::json!(3),
        };
        (bridge, auth)
    }
    fn next_frame(book: &mut StreamBook, bridge: &Bridge, auth: &Authorization, id: u64) -> Value {
        book.credit(auth, id).unwrap();
        for _ in 0..1000 {
            let mut frames = book.poll(bridge, Instant::now());
            if let Some((_, _, frame)) = frames.pop() {
                return frame;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("stream failed to produce a frame");
    }
    #[test]
    fn slow_consumer_has_no_unsolicited_values_and_completion_follows_last_item() {
        let (bridge, auth) = fixture();
        let mut book = StreamBook::default();
        book.reserve(auth.clone(), 1).unwrap();
        let (call, receiver) = bridge
            .begin_stream(
                &auth,
                crate::core::CallRequest {
                    id: 1,
                    method: "count".into(),
                    args: serde_json::json!(3),
                },
                1,
            )
            .unwrap();
        let worker = std::thread::spawn(move || call.run());
        book.attach(&auth, 1, Ok(receiver));
        std::thread::sleep(Duration::from_millis(10));
        assert!(book.poll(&bridge, Instant::now()).is_empty());
        let fixture: Vec<Value> =
            serde_json::from_str(include_str!("../tests/stream-frames.json")).unwrap();
        for expected in 0..3 {
            let frame = next_frame(&mut book, &bridge, &auth, 1);
            assert_eq!(frame["item"], expected);
            assert_eq!(frame, fixture[expected as usize]);
            assert!(frame.get("done").is_none());
            assert!(book.poll(&bridge, Instant::now()).is_empty());
        }
        let frame = next_frame(&mut book, &bridge, &auth, 1);
        assert_eq!(frame, fixture[3]);
        assert_eq!(frame["done"], true);
        assert_eq!(frame["result"]["Ok"], 3);
        assert!(book.slots.is_empty());
        worker.join().unwrap();
    }
    #[test]
    fn duplicate_credit_and_allocation_limits_are_explicit() {
        let (_, auth) = fixture();
        let mut book = StreamBook::default();
        for window in 1..=4 {
            for id in 1..=16 {
                let mut auth = auth.clone();
                auth.window = WindowId(window);
                book.reserve(auth, id).unwrap();
            }
        }
        assert_eq!(book.reserve(auth.clone(), 100), Err(CoreError::QueueFull));
        book.credit(&auth, 1).unwrap();
        assert!(matches!(
            book.credit(&auth, 1),
            Err(CoreError::InvalidArguments(_))
        ));
        book.clear(WindowId(4));
        assert_eq!(book.slots.len(), 48);
        let mut fifth = auth;
        fifth.window = WindowId(5);
        book.reserve(fifth, 1).unwrap();
    }
    #[test]
    fn idle_expiry_cancels_blocked_producer_and_reclaims_slot() {
        for waiting in [false, true] {
            let (bridge, auth) = fixture();
            let mut book = StreamBook::default();
            book.reserve(auth.clone(), 1).unwrap();
            let (call, receiver) = bridge
                .begin_stream(
                    &auth,
                    crate::core::CallRequest {
                        id: 1,
                        method: "count".into(),
                        args: serde_json::json!(100),
                    },
                    1,
                )
                .unwrap();
            let worker = std::thread::spawn(move || call.run());
            book.attach(&auth, 1, Ok(receiver));
            if waiting {
                book.credit(&auth, 1).unwrap();
            }
            let frames = book.poll(&bridge, Instant::now() + Duration::from_secs(61));
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].2["done"], true);
            assert!(
                frames[0].2["result"]["Err"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("60-second")
            );
            assert!(book.slots.is_empty());
            worker.join().unwrap();
        }
    }
    #[test]
    fn registration_errors_do_not_require_a_pull_and_stale_receivers_are_dropped() {
        let (bridge, auth) = fixture();
        let mut book = StreamBook::default();
        book.reserve(auth.clone(), 1).unwrap();
        book.attach(&auth, 1, Err(CoreError::UnknownMethod("missing".into())));
        let frames = book.poll(&bridge, Instant::now());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].2["result"]["Err"]["code"], "unknown_method");
        assert!(book.slots.is_empty());
        book.reserve(auth.clone(), 2).unwrap();
        bridge.navigate(auth.window).unwrap();
        assert!(book.poll(&bridge, Instant::now()).is_empty());
        assert!(book.slots.is_empty());
    }

    #[test]
    fn immediate_cancellation_reclaims_stream_without_a_queued_command_or_read_credit() {
        for admitted in [false, true] {
            let (bridge, auth) = fixture();
            let mut book = StreamBook::default();
            book.reserve(auth.clone(), 1).unwrap();
            let request = crate::core::CallRequest {
                id: 1,
                method: "count".into(),
                args: serde_json::json!(1),
            };
            if admitted {
                let (call, receiver) = bridge.begin_stream(&auth, request.clone(), 1).unwrap();
                call.run();
                book.attach(&auth, 1, Ok(receiver));
            }
            // The IPC handler signals the core immediately even if its bounded
            // command queue cannot admit the corresponding Cancel command.
            assert_eq!(bridge.cancel(&auth, 1), Ok(true));
            let frames = book.poll(&bridge, Instant::now());
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].2["result"]["Err"]["code"], "cancelled");
            assert!(book.slots.is_empty());
            if admitted {
                // Removal releases the core's pending request, not only the UI slot.
                assert_eq!(bridge.cancel(&auth, 1), Ok(false));
            } else {
                // Polling did not consume a tombstone needed to prevent a late
                // queued job from beginning after its UI slot was reclaimed.
                assert!(matches!(
                    bridge.begin_stream(&auth, request, 1),
                    Err(CoreError::Cancelled)
                ));
            }
        }
    }
}
