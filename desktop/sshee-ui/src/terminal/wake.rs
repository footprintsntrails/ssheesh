//! The coalescing waker: output parsed on the session's reader thread wakes the window at most
//! once per frame.
//!
//! Every call of Rune's waker posts an event to the window's loop. A `cat` of a big file is a
//! few thousand reads a second; this flag turns them into one wake until the next poll.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Rune's waker (or any other), behind a flag.
pub struct Coalesce {
    armed: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Coalesce {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Coalesce { armed: AtomicBool::new(false), wake }
    }

    /// From any thread: wakes the window unless a wake is already on its way.
    pub fn wake(&self) {
        if !self.armed.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }

    /// First thing in the window's poll, before it looks at the model: output parsed after
    /// this wakes it again. (A wake landing between this and the look costs one empty poll.)
    pub fn rearm(&self) {
        self.armed.store(false, Ordering::Release);
    }

    /// Wakes the window whatever the flag says: for the UI thread's own needs (the view found
    /// the cell size changed, and poll must fit the terminal to it), which the flag, kept for
    /// output, mustn't swallow.
    pub fn wake_now(&self) {
        (self.wake)();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn many_wakes_between_two_polls_are_one() {
        let n = Arc::new(AtomicUsize::new(0));
        let c = {
            let n = n.clone();
            Coalesce::new(Arc::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            }))
        };
        for _ in 0..1000 {
            c.wake();
        }
        assert_eq!(n.load(Ordering::SeqCst), 1);
        c.rearm();
        c.wake();
        c.wake();
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn wakes_from_many_threads() {
        let n = Arc::new(AtomicUsize::new(0));
        let c = {
            let n = n.clone();
            Arc::new(Coalesce::new(Arc::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            })))
        };
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || (0..10_000).for_each(|_| c.wake()))
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }
}
