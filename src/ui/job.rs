//! Background work for the UI: one worker thread per job, collected in `poll`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

/// Wakes the UI so its `poll` runs (Rune's waker, or nothing in tests).
#[derive(Clone)]
pub struct Wake(Option<rune::Waker>);

impl Wake {
    pub fn new(waker: Option<rune::Waker>) -> Wake {
        Wake(waker)
    }

    pub fn wake(&self) {
        if let Some(w) = &self.0 {
            w();
        }
    }
}

/// A computation running on its own thread.
///
/// Dropping a `Job` (or replacing the `Option<Job>` that holds it) raises its
/// cancel flag and drops the receiver, so stale work stops early where it
/// checks the flag and its result is discarded: no generation counters.
pub struct Job<T> {
    rx: Receiver<T>,
    cancel: Arc<AtomicBool>,
}

impl<T: Send + 'static> Job<T> {
    /// Runs `work` on a new thread. `work` gets the cancel flag and returns
    /// `None` when it gave up; a finished result wakes the UI.
    pub fn spawn<F>(wake: &Wake, work: F) -> Job<T>
    where
        F: FnOnce(&AtomicBool) -> Option<T> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let wake = wake.clone();
        std::thread::spawn(move || {
            let result = work(&flag);
            if let Some(v) = result {
                if !flag.load(Ordering::Relaxed) {
                    let _ = tx.send(v);
                }
            }
            // also wake when nothing was sent, so `poll` sees the disconnect
            wake.wake();
        });
        Job { rx, cancel }
    }
}

impl<T> Job<T> {
    /// Takes the finished result out of `slot`, leaving `None` behind. A
    /// worker that ended without a result also clears the slot.
    pub fn poll(slot: &mut Option<Job<T>>) -> Option<T> {
        let job = slot.as_ref()?;
        match job.rx.try_recv() {
            Ok(v) => {
                *slot = None;
                Some(v)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                *slot = None;
                None
            }
        }
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for<T>(slot: &mut Option<Job<T>>) -> Option<T> {
        let t0 = Instant::now();
        while slot.is_some() && t0.elapsed() < Duration::from_secs(5) {
            if let Some(v) = Job::poll(slot) {
                return Some(v);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn delivers_the_result_once() {
        let mut slot = Some(Job::spawn(&Wake::new(None), |_| Some(42)));
        assert_eq!(wait_for(&mut slot), Some(42));
        assert!(slot.is_none());
        assert_eq!(Job::poll(&mut slot), None);
    }

    #[test]
    fn dropping_the_job_cancels_the_worker() {
        let (seen_tx, seen_rx) = mpsc::channel();
        let job = Job::spawn(&Wake::new(None), move |cancel| {
            while !cancel.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
            }
            seen_tx.send(()).unwrap();
            Some(())
        });
        drop(job);
        assert!(seen_rx.recv_timeout(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn worker_without_result_clears_the_slot() {
        let mut slot: Option<Job<u32>> = Some(Job::spawn(&Wake::new(None), |_| None));
        let t0 = Instant::now();
        while slot.is_some() && t0.elapsed() < Duration::from_secs(5) {
            assert_eq!(Job::poll(&mut slot), None);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(slot.is_none());
    }

    #[test]
    fn a_finished_job_wakes_the_ui() {
        let woke = Arc::new(AtomicBool::new(false));
        let w = Arc::clone(&woke);
        let wake = Wake::new(Some(Arc::new(move || w.store(true, Ordering::Relaxed))));
        let mut slot = Some(Job::spawn(&wake, |_| Some(1)));
        assert_eq!(wait_for(&mut slot), Some(1));
        assert!(woke.load(Ordering::Relaxed));
    }
}
