//! Synchronous, display-free lifecycle contract for an application or window.
//!
//! This is a state machine for a real adapter to drive, not an OS event loop or
//! evidence that a desktop window exists. Normal startup/shutdown records
//! `Created -> Starting -> Ready -> Closing -> Closed`. Early shutdown may omit
//! `Starting` and/or `Ready`. A cancelled close does not enter `Closing`.
//!
//! State listeners run synchronously in registration order after the new state
//! is recorded. A state listener sees the current state immediately when added.
//! Readiness callbacks run once, after the `Ready` state listeners; registration
//! after readiness invokes the callback immediately, even after shutdown.
//! Callbacks must be short and may enqueue work for an adapter. They cannot
//! reenter this exclusively borrowed state machine. With panic unwinding,
//! notification panics are resumed after remaining notifications and the state
//! transition finish. A close-request panic preserves the open state and is
//! resumed after all close-request listeners run. Panic-abort builds abort.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_LIFECYCLE_ID: AtomicU64 = AtomicU64::new(1);
type PanicPayload = Box<dyn Any + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Created,
    Starting,
    Ready,
    Closing,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleError {
    pub from: LifecycleState,
    pub to: LifecycleState,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid lifecycle transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for LifecycleError {}

/// Opaque identifier shared by all listener types. Its owner identity prevents
/// a token from one lifecycle from removing a listener in another lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListenerId {
    owner: u64,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Allow,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    Cancelled,
    Closed,
}

/// A single-thread-owned lifecycle contract. Callbacks need not be `Send`.
pub struct Lifecycle {
    state: LifecycleState,
    history: Vec<LifecycleState>,
    became_ready: bool,
    owner_id: u64,
    next_listener: u64,
    state_listeners: BTreeMap<ListenerId, Box<dyn FnMut(LifecycleState)>>,
    ready_listeners: BTreeMap<ListenerId, Box<dyn FnMut()>>,
    close_listeners: BTreeMap<ListenerId, Box<dyn FnMut() -> CloseDecision>>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Lifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lifecycle")
            .field("state", &self.state)
            .field("history", &self.history)
            .field("became_ready", &self.became_ready)
            .field("state_listener_count", &self.state_listeners.len())
            .field("ready_listener_count", &self.ready_listeners.len())
            .field("close_listener_count", &self.close_listeners.len())
            .finish()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            state: LifecycleState::Created,
            history: vec![LifecycleState::Created],
            became_ready: false,
            owner_id: allocate_owner_id(),
            next_listener: 1,
            state_listeners: BTreeMap::new(),
            ready_listeners: BTreeMap::new(),
            close_listeners: BTreeMap::new(),
        }
    }

    pub fn state(&self) -> LifecycleState {
        self.state
    }

    /// Ordered state transitions, including the initial `Created` state.
    pub fn history(&self) -> &[LifecycleState] {
        &self.history
    }

    pub fn has_become_ready(&self) -> bool {
        self.became_ready
    }

    pub fn start(&mut self) -> Result<(), LifecycleError> {
        self.require_state(LifecycleState::Created, LifecycleState::Starting)?;
        let panic = self.transition(LifecycleState::Starting);
        resume_first_panic(panic);
        Ok(())
    }

    /// Marks the adapter ready exactly once. A second call is an error.
    pub fn mark_ready(&mut self) -> Result<(), LifecycleError> {
        self.require_state(LifecycleState::Starting, LifecycleState::Ready)?;
        self.became_ready = true;
        let mut panic = self.transition(LifecycleState::Ready);
        let callbacks = std::mem::take(&mut self.ready_listeners);
        for (_, mut callback) in callbacks {
            remember_first_panic(
                &mut panic,
                catch_unwind(AssertUnwindSafe(&mut callback)).err(),
            );
        }
        resume_first_panic(panic);
        Ok(())
    }

    /// Requests ordinary shutdown. Every close listener runs, in registration
    /// order. Any cancellation wins, regardless of later listeners' decisions.
    /// A cancellation retains the exact prior state and all listeners.
    pub fn request_close(&mut self) -> Result<CloseOutcome, LifecycleError> {
        self.require_open()?;
        let mut cancelled = false;
        let mut panic = None;
        for callback in self.close_listeners.values_mut() {
            match catch_unwind(AssertUnwindSafe(callback)) {
                Ok(CloseDecision::Cancel) => cancelled = true,
                Ok(CloseDecision::Allow) => {}
                Err(payload) => remember_first_panic(&mut panic, Some(payload)),
            }
        }
        resume_first_panic(panic);
        if cancelled {
            return Ok(CloseOutcome::Cancelled);
        }
        self.finish_close();
        Ok(CloseOutcome::Closed)
    }

    /// Forces shutdown without consulting close-request listeners. State
    /// listeners still observe `Closing` followed by `Closed`.
    pub fn destroy(&mut self) -> Result<(), LifecycleError> {
        self.require_open()?;
        self.finish_close();
        Ok(())
    }

    /// Immediately reports the current state, then subscribes to transitions.
    /// A listener added after closure is invoked once and is not retained.
    pub fn on_state_change(
        &mut self,
        mut callback: impl FnMut(LifecycleState) + 'static,
    ) -> ListenerId {
        let id = self.allocate_listener();
        callback(self.state);
        if self.state != LifecycleState::Closed {
            self.state_listeners.insert(id, Box::new(callback));
        }
        id
    }

    /// Subscribes to the one-time readiness notification. Once readiness has
    /// happened, invokes the callback before returning without retaining it.
    /// If closed before readiness, the callback is discarded without running.
    pub fn on_ready(&mut self, mut callback: impl FnMut() + 'static) -> ListenerId {
        let id = self.allocate_listener();
        if self.became_ready {
            callback();
        } else if self.state != LifecycleState::Closed {
            self.ready_listeners.insert(id, Box::new(callback));
        }
        id
    }

    /// Subscribes to cancellable close attempts. A closed lifecycle does not
    /// retain new close listeners; its close operations always return errors.
    pub fn on_close_request(
        &mut self,
        callback: impl FnMut() -> CloseDecision + 'static,
    ) -> ListenerId {
        let id = self.allocate_listener();
        if self.state != LifecycleState::Closed {
            self.close_listeners.insert(id, Box::new(callback));
        }
        id
    }

    /// Returns whether a live listener was removed. One-shot ready listeners
    /// have already been removed after firing. IDs belong to their lifecycle.
    pub fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.state_listeners.remove(&id).is_some()
            || self.ready_listeners.remove(&id).is_some()
            || self.close_listeners.remove(&id).is_some()
    }

    fn allocate_listener(&mut self) -> ListenerId {
        let id = ListenerId {
            owner: self.owner_id,
            sequence: self.next_listener,
        };
        self.next_listener = self
            .next_listener
            .checked_add(1)
            .expect("lifecycle listener identifier space exhausted");
        id
    }

    fn require_state(
        &self,
        expected: LifecycleState,
        to: LifecycleState,
    ) -> Result<(), LifecycleError> {
        if self.state == expected {
            Ok(())
        } else {
            Err(LifecycleError {
                from: self.state,
                to,
            })
        }
    }

    fn require_open(&self) -> Result<(), LifecycleError> {
        match self.state {
            LifecycleState::Created | LifecycleState::Starting | LifecycleState::Ready => Ok(()),
            _ => Err(LifecycleError {
                from: self.state,
                to: LifecycleState::Closing,
            }),
        }
    }

    fn transition(&mut self, state: LifecycleState) -> Option<PanicPayload> {
        self.state = state;
        self.history.push(state);
        let mut panic = None;
        for callback in self.state_listeners.values_mut() {
            remember_first_panic(
                &mut panic,
                catch_unwind(AssertUnwindSafe(|| callback(state))).err(),
            );
        }
        panic
    }

    fn finish_close(&mut self) {
        let mut panic = self.transition(LifecycleState::Closing);
        remember_first_panic(&mut panic, self.transition(LifecycleState::Closed));
        self.state_listeners.clear();
        self.ready_listeners.clear();
        self.close_listeners.clear();
        resume_first_panic(panic);
    }
}

fn allocate_owner_id() -> u64 {
    let mut current = NEXT_LIFECYCLE_ID.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .expect("lifecycle owner identifier space exhausted");
        match NEXT_LIFECYCLE_ID.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return current,
            Err(actual) => current = actual,
        }
    }
}

fn remember_first_panic(first: &mut Option<PanicPayload>, next: Option<PanicPayload>) {
    if first.is_none() {
        *first = next;
    }
}

fn resume_first_panic(panic: Option<PanicPayload>) {
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn complete_order_and_exactly_once_readiness() {
        let mut lifecycle = Lifecycle::new();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&observed);
        lifecycle.on_state_change(move |state| seen.borrow_mut().push(state));
        let ready_count = Rc::new(Cell::new(0));
        let count = Rc::clone(&ready_count);
        let id = lifecycle.on_ready(move || count.set(count.get() + 1));
        lifecycle.start().unwrap();
        lifecycle.mark_ready().unwrap();
        assert!(lifecycle.mark_ready().is_err());
        assert_eq!(ready_count.get(), 1);
        assert!(!lifecycle.remove_listener(id));
        assert_eq!(lifecycle.request_close().unwrap(), CloseOutcome::Closed);
        let expected = [
            LifecycleState::Created,
            LifecycleState::Starting,
            LifecycleState::Ready,
            LifecycleState::Closing,
            LifecycleState::Closed,
        ];
        assert_eq!(lifecycle.history(), expected);
        assert_eq!(observed.borrow().as_slice(), expected);
    }

    #[test]
    fn late_ready_runs_before_registration_returns_even_after_closure() {
        let mut lifecycle = Lifecycle::new();
        lifecycle.start().unwrap();
        lifecycle.mark_ready().unwrap();
        let count = Rc::new(Cell::new(0));
        let copy = Rc::clone(&count);
        let id = lifecycle.on_ready(move || copy.set(copy.get() + 1));
        assert_eq!(count.get(), 1);
        assert!(!lifecycle.remove_listener(id));
        lifecycle.destroy().unwrap();
        let copy = Rc::clone(&count);
        lifecycle.on_ready(move || copy.set(copy.get() + 1));
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn first_close_can_be_cancelled_and_second_allowed() {
        let mut lifecycle = Lifecycle::new();
        lifecycle.start().unwrap();
        lifecycle.mark_ready().unwrap();
        let attempts = Rc::new(Cell::new(0));
        let copy = Rc::clone(&attempts);
        lifecycle.on_close_request(move || {
            copy.set(copy.get() + 1);
            if copy.get() == 1 {
                CloseDecision::Cancel
            } else {
                CloseDecision::Allow
            }
        });
        assert_eq!(lifecycle.request_close().unwrap(), CloseOutcome::Cancelled);
        assert_eq!(lifecycle.state(), LifecycleState::Ready);
        assert_eq!(lifecycle.history().len(), 3);
        assert_eq!(lifecycle.request_close().unwrap(), CloseOutcome::Closed);
        assert_eq!(attempts.get(), 2);
        assert_eq!(lifecycle.state(), LifecycleState::Closed);
        assert!(lifecycle.request_close().is_err());
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn close_cancellation_is_sticky_and_all_listeners_run_in_order() {
        let mut lifecycle = Lifecycle::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        for (number, decision) in [(1, CloseDecision::Cancel), (2, CloseDecision::Allow)] {
            let copy = Rc::clone(&seen);
            lifecycle.on_close_request(move || {
                copy.borrow_mut().push(number);
                decision
            });
        }
        assert_eq!(lifecycle.request_close().unwrap(), CloseOutcome::Cancelled);
        assert_eq!(seen.borrow().as_slice(), &[1, 2]);
        assert_eq!(lifecycle.state(), LifecycleState::Created);
    }

    #[test]
    fn destruction_bypasses_cancellation_but_reports_shutdown_states() {
        let mut lifecycle = Lifecycle::new();
        lifecycle.on_close_request(|| panic!("destroy must bypass close callbacks"));
        lifecycle.destroy().unwrap();
        assert_eq!(
            lifecycle.history(),
            &[
                LifecycleState::Created,
                LifecycleState::Closing,
                LifecycleState::Closed
            ]
        );
        assert!(lifecycle.destroy().is_err());
    }

    #[test]
    fn every_listener_kind_can_be_removed_before_it_runs_again() {
        let mut lifecycle = Lifecycle::new();
        let states = Rc::new(Cell::new(0));
        let copy = Rc::clone(&states);
        let state_id = lifecycle.on_state_change(move |_| copy.set(copy.get() + 1));
        let ready_id = lifecycle.on_ready(|| panic!("removed readiness listener"));
        let close_id = lifecycle.on_close_request(|| panic!("removed close listener"));
        for id in [state_id, ready_id, close_id] {
            assert!(lifecycle.remove_listener(id));
            assert!(!lifecycle.remove_listener(id));
        }
        lifecycle.start().unwrap();
        lifecycle.mark_ready().unwrap();
        lifecycle.request_close().unwrap();
        assert_eq!(states.get(), 1);
    }

    #[test]
    fn invalid_transitions_leave_history_and_readiness_unchanged() {
        let mut lifecycle = Lifecycle::new();
        assert_eq!(
            lifecycle.mark_ready(),
            Err(LifecycleError {
                from: LifecycleState::Created,
                to: LifecycleState::Ready
            })
        );
        assert_eq!(lifecycle.history(), &[LifecycleState::Created]);
        assert!(!lifecycle.has_become_ready());
        lifecycle.start().unwrap();
        assert!(lifecycle.start().is_err());
        lifecycle.destroy().unwrap();
        assert!(lifecycle.start().is_err());
        assert!(lifecycle.mark_ready().is_err());
        assert_eq!(
            lifecycle.history(),
            &[
                LifecycleState::Created,
                LifecycleState::Starting,
                LifecycleState::Closing,
                LifecycleState::Closed
            ]
        );
        lifecycle.on_ready(|| panic!("closed-before-ready cannot become ready"));
        assert!(!lifecycle.has_become_ready());
    }

    #[test]
    fn ready_callbacks_follow_state_callbacks_in_registration_order() {
        let mut lifecycle = Lifecycle::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        for number in [1, 2] {
            let copy = Rc::clone(&seen);
            lifecycle.on_state_change(move |state| {
                if state == LifecycleState::Ready {
                    copy.borrow_mut().push(number);
                }
            });
        }
        for number in [3, 4] {
            let copy = Rc::clone(&seen);
            lifecycle.on_ready(move || copy.borrow_mut().push(number));
        }
        lifecycle.start().unwrap();
        lifecycle.mark_ready().unwrap();
        assert_eq!(seen.borrow().as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn independent_lifecycles_do_not_share_state() {
        let mut first = Lifecycle::new();
        let mut second = Lifecycle::new();
        first.start().unwrap();
        second.start().unwrap();
        first.mark_ready().unwrap();
        second.mark_ready().unwrap();
        first.destroy().unwrap();
        assert_eq!(second.state(), LifecycleState::Ready);
        assert!(second.has_become_ready());
        assert_eq!(second.request_close().unwrap(), CloseOutcome::Closed);
    }

    #[test]
    fn a_foreign_listener_token_cannot_remove_a_local_listener() {
        let mut first = Lifecycle::new();
        let mut second = Lifecycle::new();
        let foreign = first.on_ready(|| {});
        let invoked = Rc::new(Cell::new(false));
        let copy = Rc::clone(&invoked);
        let local = second.on_ready(move || copy.set(true));
        assert_ne!(foreign, local);
        assert!(!second.remove_listener(foreign));
        second.start().unwrap();
        second.mark_ready().unwrap();
        assert!(invoked.get());
    }

    #[test]
    fn readiness_notifications_finish_before_resuming_a_callback_panic() {
        let mut lifecycle = Lifecycle::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        lifecycle.on_state_change(|state| {
            if state == LifecycleState::Ready {
                panic!("state observer failed");
            }
        });
        let copy = Rc::clone(&seen);
        lifecycle.on_ready(move || {
            copy.borrow_mut().push(1);
            panic!("ready observer failed");
        });
        let copy = Rc::clone(&seen);
        lifecycle.on_ready(move || copy.borrow_mut().push(2));
        lifecycle.start().unwrap();
        let panic = catch_unwind(AssertUnwindSafe(|| lifecycle.mark_ready()));
        assert!(panic.is_err());
        assert_eq!(lifecycle.state(), LifecycleState::Ready);
        assert_eq!(seen.borrow().as_slice(), &[1, 2]);
        assert!(lifecycle.mark_ready().is_err());
    }

    #[test]
    fn shutdown_finishes_before_resuming_a_state_callback_panic() {
        let mut lifecycle = Lifecycle::new();
        lifecycle.on_state_change(|state| {
            if state == LifecycleState::Closing {
                panic!("closing observer failed");
            }
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let copy = Rc::clone(&seen);
        let id = lifecycle.on_state_change(move |state| copy.borrow_mut().push(state));
        let panic = catch_unwind(AssertUnwindSafe(|| lifecycle.destroy()));
        assert!(panic.is_err());
        assert_eq!(lifecycle.state(), LifecycleState::Closed);
        assert_eq!(
            seen.borrow().as_slice(),
            &[
                LifecycleState::Created,
                LifecycleState::Closing,
                LifecycleState::Closed
            ]
        );
        assert!(!lifecycle.remove_listener(id));
    }

    #[test]
    fn close_request_panic_preserves_state_and_runs_remaining_listeners() {
        let mut lifecycle = Lifecycle::new();
        let id = lifecycle.on_close_request(|| panic!("close observer failed"));
        let invoked = Rc::new(Cell::new(false));
        let copy = Rc::clone(&invoked);
        lifecycle.on_close_request(move || {
            copy.set(true);
            CloseDecision::Allow
        });
        let panic = catch_unwind(AssertUnwindSafe(|| lifecycle.request_close()));
        assert!(panic.is_err());
        assert!(invoked.get());
        assert_eq!(lifecycle.state(), LifecycleState::Created);
        assert!(lifecycle.remove_listener(id));
        assert_eq!(lifecycle.request_close().unwrap(), CloseOutcome::Closed);
    }
}
