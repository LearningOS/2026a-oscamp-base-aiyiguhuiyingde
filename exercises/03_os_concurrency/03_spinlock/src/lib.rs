//! # Spin Lock
//!
//! In this exercise, you will implement a basic spin lock.
//! Spin locks are one of the most fundamental synchronization primitives in OS kernels.
//!
//! ## Key Concepts
//! - Spin locks use busy-waiting to acquire the lock
//! - `AtomicBool`'s `compare_exchange` to implement lock acquisition
//! - `core::hint::spin_loop` to reduce CPU power consumption
//! - `UnsafeCell` provides interior mutability

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Basic spin lock
pub struct SpinLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquire lock, returning a mutable reference to inner data.
    ///
    /// TODO: Use compare_exchange to spin until lock is acquired
    /// 1. In a loop, try to change locked from false to true
    /// 2. Success uses Acquire ordering, failure uses Relaxed
    /// 3. On failure call `core::hint::spin_loop()` to hint CPU
    /// 4. On success return `&mut *self.data.get()`
    ///
    /// # Safety
    /// Caller must ensure `unlock` is called after using the data.
    pub fn lock(&self) -> &mut T {
        // compare_exchange_weak is the canonical spin-lock primitive: it may
        // fail spuriously (cheap on LL/SC machines such as ARM/PowerPC) and we
        // simply retry. Success uses Acquire so that everything the previous
        // owner did inside its critical section becomes visible to us; failure
        // only needs Relaxed because we learned nothing yet.
        //
        // Bounded backoff on contention. Deliberately no
        // `std::thread::yield_now()`: a leaked guard (`mem::forget`) would then
        // spin forever while every other thread blocked, which is unacceptable
        // for a kernel-style primitive.
        let mut spins: u32 = 0;
        loop {
            match self.locked.compare_exchange_weak(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_previous_false) => {
                    // SAFETY: `locked == true` and we are the only thread that
                    // observed it, so no other holder exists; handing out an
                    // exclusive &mut until unlock() is the lock's contract.
                    return unsafe { &mut *self.data.get() };
                }
                Err(_) => {
                    let hints = if spins < 8 { 1 } else { spins.min(64) };
                    for _ in 0..hints {
                        core::hint::spin_loop();
                    }
                    spins += 1;
                }
            }
        }
    }

    /// Release lock.
    ///
    /// TODO: Set locked to false (using Release ordering)
    pub fn unlock(&self) {
        // Release: our critical-section writes must be published before the lock
        // becomes observable as free, pairing with the next holder's Acquire.
        self.locked.store(false, Ordering::Release);
    }

    /// Try to acquire lock without spinning.
    /// Returns Some(&mut T) on success, None if lock is busy.
    pub fn try_lock(&self) -> Option<&mut T> {
        // One attempt only; strong CAS so a spurious failure would wrongly
        // report "busy" when the lock is actually free.
        match self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_previous_false) => Some(unsafe { &mut *self.data.get() }),
            Err(_busy) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_basic_lock_unlock() {
        let lock = SpinLock::new(0u32);
        {
            let data = lock.lock();
            *data = 42;
            lock.unlock();
        }
        let data = lock.lock();
        assert_eq!(*data, 42);
        lock.unlock();
    }

    #[test]
    fn test_try_lock() {
        let lock = SpinLock::new(0u32);
        assert!(lock.try_lock().is_some());
        lock.unlock();
    }

    #[test]
    fn test_concurrent_counter() {
        let lock = Arc::new(SpinLock::new(0u64));
        let mut handles = vec![];

        for _ in 0..10 {
            let l = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    let data = l.lock();
                    *data += 1;
                    l.unlock();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let data = lock.lock();
        assert_eq!(*data, 10000);
        lock.unlock();
    }

    #[test]
    fn test_lock_protects_data() {
        let lock = Arc::new(SpinLock::new(Vec::new()));
        let mut handles = vec![];

        for i in 0..5 {
            let l = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                let data = l.lock();
                data.push(i);
                l.unlock();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let data = lock.lock();
        let mut sorted = data.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
        lock.unlock();
    }
}
