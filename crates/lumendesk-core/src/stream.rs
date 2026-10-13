//! Bounded, ordered streams. Full queues reject or suspend the producer rather
//! than allocating more memory. The consumer drains values before completion.
//! Dropping the receiver cancels its token, including a shared call token.
use crate::{CancellationToken, CoreError, Result, Schema};
use serde::Serialize;
use serde_json::Value;
use std::marker::PhantomData;
use std::time::Duration;
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};

#[derive(Debug, Clone, PartialEq)]
pub enum StreamItem<T> {
    Value { sequence: u64, value: T },
    Complete(Result<()>),
}

struct State<T> {
    queue: VecDeque<(u64, T)>,
    capacity: usize,
    next_sequence: u64,
    completion: Option<Result<()>>,
    completion_read: bool,
    call_managed: bool,
}

struct Shared<T> {
    inner: Mutex<State<T>>,
    changed: Condvar,
}

pub struct StreamSender<T> {
    state: Arc<Shared<T>>,
    cancellation: CancellationToken,
}
pub struct StreamReceiver<T> {
    state: Arc<Shared<T>>,
    cancellation: CancellationToken,
}

pub fn bounded_stream<T>(
    capacity: usize,
    cancellation: CancellationToken,
) -> Result<(StreamSender<T>, StreamReceiver<T>)> {
    if capacity == 0 {
        return Err(CoreError::InvalidConfiguration(
            "stream capacity must be positive".into(),
        ));
    }
    let state = Arc::new(Shared {
        inner: Mutex::new(State {
            queue: VecDeque::new(),
            capacity,
            next_sequence: 0,
            completion: None,
            completion_read: false,
            call_managed: false,
        }),
        changed: Condvar::new(),
    });
    Ok((
        StreamSender {
            state: state.clone(),
            cancellation: cancellation.clone(),
        },
        StreamReceiver {
            state,
            cancellation,
        },
    ))
}

impl<T> StreamSender<T> {
    pub fn send(&self, value: T) -> Result<u64> {
        self.cancellation.check()?;
        let mut state = self.state.inner.lock().unwrap();
        self.cancellation.check()?;
        if state.completion.is_some() {
            return Err(CoreError::StreamFinished);
        }
        if state.queue.len() == state.capacity {
            return Err(CoreError::QueueFull);
        }
        let sequence = state.next_sequence;
        state.next_sequence = sequence.checked_add(1).ok_or(CoreError::StreamFinished)?;
        state.queue.push_back((sequence, value));
        Ok(sequence)
    }
    /// Blocks only the producer thread while full. Values are retained until admitted;
    /// receiver consumption wakes the producer, and cancellation is checked at least
    /// every 25 ms even when it originates from an external session token.
    pub fn send_wait(&self, value: T) -> Result<u64> {
        let mut state = self.state.inner.lock().unwrap();
        loop {
            self.cancellation.check()?;
            if state.completion.is_some() {
                return Err(CoreError::StreamFinished);
            }
            if state.queue.len() < state.capacity {
                let sequence = state.next_sequence;
                state.next_sequence = sequence.checked_add(1).ok_or(CoreError::StreamFinished)?;
                state.queue.push_back((sequence, value));
                return Ok(sequence);
            }
            state = self
                .state
                .changed
                .wait_timeout(state, Duration::from_millis(25))
                .unwrap()
                .0;
        }
    }
    /// Completion has a separate slot and never needs queue capacity.
    pub fn finish(&self, result: Result<()>) -> Result<()> {
        self.cancellation.check()?;
        let mut state = self.state.inner.lock().unwrap();
        self.cancellation.check()?;
        if state.completion.is_some() {
            return Err(CoreError::StreamFinished);
        }
        state.completion = Some(result);
        self.state.changed.notify_all();
        Ok(())
    }
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
    pub fn queued_len(&self) -> usize {
        self.state.inner.lock().unwrap().queue.len()
    }
}

impl<T: Send + 'static> StreamSender<T> {
    pub(crate) fn completion_hook(&self) -> Box<dyn FnOnce(Result<()>) + Send> {
        let state = self.state.clone();
        state.inner.lock().unwrap().call_managed = true;
        let shared = state;
        Box::new(move |result| {
            let mut state = shared.inner.lock().unwrap();
            if state.completion.is_none() {
                state.completion = Some(result);
            }
            drop(state);
            shared.changed.notify_all();
        })
    }
}

impl<T> Drop for StreamSender<T> {
    fn drop(&mut self) {
        let mut state = self.state.inner.lock().unwrap();
        if state.completion.is_none() && !state.call_managed {
            state.completion = Some(Err(CoreError::Backend(
                "stream producer ended without completion".into(),
            )));
        }
    }
}

impl<T> StreamReceiver<T> {
    /// `None` means no item is currently available or completion was consumed.
    /// Cancellation discards all outstanding values and remains an error.
    pub fn try_next(&self) -> Result<Option<StreamItem<T>>> {
        let mut state = self.state.inner.lock().unwrap();
        if self.cancellation.is_cancelled() {
            state.queue.clear();
            return Err(CoreError::Cancelled);
        }
        if let Some((sequence, value)) = state.queue.pop_front() {
            self.state.changed.notify_one();
            return Ok(Some(StreamItem::Value { sequence, value }));
        }
        if state.completion_read {
            return Ok(None);
        }
        if let Some(result) = state.completion.clone() {
            state.completion_read = true;
            return Ok(Some(StreamItem::Complete(result)));
        }
        Ok(None)
    }
    pub fn queued_len(&self) -> usize {
        self.state.inner.lock().unwrap().queue.len()
    }
    pub fn cancel(&self) {
        self.cancellation.cancel();
        self.state.inner.lock().unwrap().queue.clear();
        self.state.changed.notify_all();
    }
    pub fn is_complete(&self) -> bool {
        self.state.inner.lock().unwrap().completion_read
    }
}

impl<T> Drop for StreamReceiver<T> {
    fn drop(&mut self) {
        if !self.is_complete() {
            self.cancel();
        }
    }
}

/// Typed frontend producer. Each item is serialized, schema-checked and size-limited
/// before admission to the bounded call stream. `QueueFull` is deliberate backpressure;
/// retry only after allowing the host consumer to drain. Completion belongs to the
/// handler return value, so this producer cannot finish a frontend stream early.
pub struct StreamSink<T> {
    sender: StreamSender<Value>,
    schema: Schema,
    max_payload_bytes: usize,
    marker: PhantomData<fn(T)>,
}

impl<T: Serialize> StreamSink<T> {
    pub(crate) fn new(
        sender: StreamSender<Value>,
        schema: Schema,
        max_payload_bytes: usize,
    ) -> Self {
        Self {
            sender,
            schema,
            max_payload_bytes,
            marker: PhantomData,
        }
    }
    pub fn send(&self, item: T) -> Result<u64> {
        self.sender.send(self.encode(item)?)
    }
    /// Wait for capacity without dropping the item. Run on a backend worker,
    /// never the UI thread; cancellation interrupts the wait.
    pub fn send_wait(&self, item: T) -> Result<u64> {
        self.sender.send_wait(self.encode(item)?)
    }
    fn encode(&self, item: T) -> Result<Value> {
        self.sender.cancellation().check()?;
        let value = serde_json::to_value(item)
            .map_err(|error| CoreError::InvalidResult(error.to_string()))?;
        self.schema
            .validate(&value)
            .map_err(CoreError::InvalidResult)?;
        let bytes = serde_json::to_vec(&value)
            .map_err(|error| CoreError::InvalidResult(error.to_string()))?;
        if bytes.len() > self.max_payload_bytes {
            return Err(CoreError::PayloadTooLarge);
        }
        Ok(value)
    }
    pub fn cancellation(&self) -> &CancellationToken {
        self.sender.cancellation()
    }
    pub fn queued_len(&self) -> usize {
        self.sender.queued_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_and_ordered_before_completion() {
        let (tx, rx) = bounded_stream(2, CancellationToken::new()).unwrap();
        assert_eq!(tx.send(10), Ok(0));
        assert_eq!(tx.send(20), Ok(1));
        assert_eq!(tx.send(30), Err(CoreError::QueueFull));
        assert_eq!(tx.queued_len(), 2);
        tx.finish(Ok(())).unwrap();
        assert_eq!(tx.send(40), Err(CoreError::StreamFinished));
        assert_eq!(
            rx.try_next().unwrap(),
            Some(StreamItem::Value {
                sequence: 0,
                value: 10
            })
        );
        assert_eq!(
            rx.try_next().unwrap(),
            Some(StreamItem::Value {
                sequence: 1,
                value: 20
            })
        );
        assert_eq!(rx.try_next().unwrap(), Some(StreamItem::Complete(Ok(()))));
        assert_eq!(rx.try_next().unwrap(), None);
        assert!(rx.is_complete());
    }
    #[test]
    fn consumer_leaving_cancels_producer() {
        let token = CancellationToken::new();
        let (tx, rx) = bounded_stream(1, token.clone()).unwrap();
        drop(rx);
        assert!(token.is_cancelled());
        assert_eq!(tx.send(1), Err(CoreError::Cancelled));
    }
    #[test]
    fn navigation_cancellation_discards_buffered_values() {
        let token = CancellationToken::new();
        let (tx, rx) = bounded_stream(4, token.clone()).unwrap();
        tx.send(1).unwrap();
        token.cancel();
        assert_eq!(rx.try_next(), Err(CoreError::Cancelled));
        assert_eq!(tx.queued_len(), 0);
    }
    #[test]
    fn unfinished_producer_reports_error_after_values() {
        let (tx, rx) = bounded_stream(1, CancellationToken::new()).unwrap();
        tx.send("one").unwrap();
        drop(tx);
        assert!(matches!(
            rx.try_next().unwrap(),
            Some(StreamItem::Value { sequence: 0, .. })
        ));
        assert!(matches!(
            rx.try_next().unwrap(),
            Some(StreamItem::Complete(Err(CoreError::Backend(_))))
        ));
    }
    #[test]
    fn rejected_sends_do_not_skip_sequence_numbers() {
        let (tx, rx) = bounded_stream(1, CancellationToken::new()).unwrap();
        tx.send(1).unwrap();
        assert_eq!(tx.send(2), Err(CoreError::QueueFull));
        rx.try_next().unwrap();
        assert_eq!(tx.send(3), Ok(1));
    }
}
