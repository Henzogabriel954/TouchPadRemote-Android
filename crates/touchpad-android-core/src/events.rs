use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::models::UiEvent;

const MAX_QUEUED_EVENTS: usize = 512;

/// Zero-polling `Condvar`-backed UI event queue for delivering Rust events to Kotlin.
#[derive(Debug, Default)]
pub struct EventBus {
    queue: Mutex<VecDeque<UiEvent>>,
    condvar: Condvar,
    shutdown: AtomicBool,
}

impl EventBus {
    /// Creates an empty event bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues an event, coalescing consecutive `StateUpdated` notifications, and wakes waiters.
    pub fn push(&self, event: UiEvent) {
        let mut guard = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if matches!(event, UiEvent::StateUpdated)
            && matches!(guard.back(), Some(UiEvent::StateUpdated))
        {
            self.condvar.notify_all();
            return;
        }

        if guard.len() >= MAX_QUEUED_EVENTS {
            guard.pop_front();
        }
        guard.push_back(event);
        self.condvar.notify_all();
    }

    /// Drains pending events immediately if non-empty, or blocks until an event arrives,
    /// `timeout_ms` elapses, or the bus is shut down.
    pub fn wait_events(&self, timeout_ms: u64) -> Vec<UiEvent> {
        let mut guard = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if !guard.is_empty() {
            return guard.drain(..).collect();
        }

        if timeout_ms == 0 || self.shutdown.load(Ordering::Acquire) {
            return Vec::new();
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while guard.is_empty() && !self.shutdown.load(Ordering::Acquire) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, wait_res) = self
                .condvar
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = next_guard;
            if wait_res.timed_out() && guard.is_empty() {
                break;
            }
        }

        guard.drain(..).collect()
    }

    /// Waits for events and serializes the resulting list as JSON (falling back to `"[]"`).
    pub fn wait_events_json(&self, timeout_ms: u64) -> String {
        let events = self.wait_events(timeout_ms);
        serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string())
    }

    /// Marks the bus as shut down and wakes all blocked waiters immediately.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        let _guard = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.condvar.notify_all();
    }
}
