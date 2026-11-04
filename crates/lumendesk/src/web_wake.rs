//! Bounded wake coalescing and a stoppable idle-loop ticker (no busy polling).
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default)]
pub(crate) struct WakeSignal(Arc<AtomicBool>);
impl WakeSignal {
    pub fn request(&self, send: impl FnOnce() -> bool) -> bool {
        if self.0.swap(true, Ordering::AcqRel) {
            return true;
        }
        if send() {
            true
        } else {
            self.0.store(false, Ordering::Release);
            false
        }
    }
    pub fn acknowledge(&self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(target_os = "linux")]
pub(crate) struct WakeTicker {
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[cfg(target_os = "linux")]
impl WakeTicker {
    pub fn new(interval: std::time::Duration, wake: impl Fn() -> bool + Send + 'static) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = std::thread::spawn(move || {
            loop {
                std::thread::park_timeout(interval);
                if stop.load(Ordering::Acquire) || !wake() {
                    break;
                }
            }
        });
        Self {
            stopped,
            worker: Some(worker),
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for WakeTicker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_requests_are_coalesced_until_acknowledged() {
        let signal = WakeSignal::default();
        assert!(signal.request(|| true));
        assert!(signal.request(|| panic!("duplicate event must not be sent")));
        signal.acknowledge();
        let mut sent = false;
        assert!(signal.request(|| {
            sent = true;
            true
        }));
        assert!(sent);
    }
    #[test]
    fn failed_send_does_not_leave_a_pending_wake() {
        let signal = WakeSignal::default();
        assert!(!signal.request(|| false));
        assert!(signal.request(|| true));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn idle_ticker_wakes_and_drop_stops_and_joins_it() {
        use std::{sync::mpsc, time::Duration};
        let (tx, rx) = mpsc::channel();
        let ticker = WakeTicker::new(Duration::from_millis(5), move || tx.send(()).is_ok());
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(ticker);
        // Joining drops the sender; drain any tick that preceded shutdown.
        while rx.try_recv().is_ok() {}
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
    }
}
