//! # Read-Write Lock (Writer-Priority)
//!
//! In this exercise, you will implement a **writer-priority** read-write lock from scratch using atomics.
//! Multiple readers may hold the lock concurrently; a writer holds it exclusively.
//!
//! **Note:** Rust's standard library already provides [`std::sync::RwLock`]. This exercise implements
//! a minimal version for learning the protocol and policy without using the standard one.
//!
//! ## Common policies for read-write locks
//! Different implementations can give different **priority** when both readers and writers are waiting:
//!
//! - **Reader-priority (读者优先)**: New readers are allowed to enter while a writer is waiting, so writers
//!   may be starved if readers keep arriving.
//! - **Writer-priority (写者优先)**: Once a writer is waiting, no new readers are admitted until that writer
//!   has run; this exercise implements this policy.
//! - **Read-write fair (读写公平)**: Requests are served in a fair order (e.g. FIFO or round-robin), so
//!   neither readers nor writers are systematically starved.
//!
//! ## Key Concepts
//! - **Readers**: share access; many threads can hold a read lock at once.
//! - **Writer**: exclusive access; only one writer, and no readers while the writer holds the lock.
//! - **Writer-priority (this implementation)**: when at least one writer is waiting, new readers block
//!   until the writer runs.
//!
//! ## State (single atomic)
//! We use one `AtomicU32`: low bits = reader count, two flags = writer holding / writer waiting.
//! All logic is implemented with compare_exchange and load/store; no use of `std::sync::RwLock`.

use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};

static CLAIMS: AtomicU32 = AtomicU32::new(0);
static FASTC: AtomicU32 = AtomicU32::new(0);
static ELSEC: AtomicU32 = AtomicU32::new(0);
static ZEROC: AtomicU32 = AtomicU32::new(0);

/// Maximum number of concurrent readers (fits in state bits).
const READER_MASK: u32 = (1 << 30) - 1;
/// Bit set when a writer holds the lock.
const WRITER_HOLDING: u32 = 1 << 30;
/// Bit set when at least one writer is waiting (writer-priority: block new readers).
const WRITER_WAITING: u32 = 1 << 31;

/// Writer-priority read-write lock. Implemented from scratch; does not use `std::sync::RwLock`.
pub struct RwLock<T> {
    state: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for RwLock<T> {}
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

// Bounded spin backoff: after this many failed attempts we emit an extra round
// of `spin_loop()` hints. It is deliberately *not* `std::thread::yield_now()`:
// yielding can hand the CPU to a thread that will never run again (e.g. one
// whose guard was leaked), which would wedge a kernel-style spin lock.
const BACKOFF_MAX: u32 = 64;

/// Returns the number of `spin_loop()` hints to emit for iteration `n`.
#[inline]
fn backoff_spins(n: u32) -> u32 {
    if n < 8 {
        1
    } else {
        n.min(BACKOFF_MAX)
    }
}

impl<T> RwLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquire a read lock. Blocks (spins) until no writer holds and no writer is waiting (writer-priority).
    ///
    /// TODO: Implement read lock acquisition
    /// 1. In a loop, load state (Acquire).
    /// 2. If WRITER_HOLDING or WRITER_WAITING is set, spin_loop and continue (writer-priority: no new readers while writer waits).
    /// 3. If reader count (state & READER_MASK) is already READER_MASK, spin and continue.
    /// 4. Try compare_exchange(s, s + 1, AcqRel, Acquire); on success return RwLockReadGuard { lock: self }.
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        let mut spins: u32 = 0;
        loop {
            // Acquire: if we end up succeeding, this load is part of the edge
            // that publishes the previous writer's data to us.
            let s = self.state.load(Ordering::Acquire);

            // Writer-priority policy: while a writer holds *or merely waits*, no
            // new reader is admitted. That is what prevents writer starvation --
            // an endless stream of readers cannot keep a waiting writer out.
            if spins > 2_000_000 { eprintln!("R-STUCK s={:#010x}", s); spins = 0; }
            if s & (WRITER_HOLDING | WRITER_WAITING) != 0 {
                for _ in 0..backoff_spins(spins) {
                    core::hint::spin_loop();
                }
                spins += 1;
                continue;
            }

            // Guard against overflowing into the writer bit: refuse to add one
            // more reader once the counter field is saturated.
            if (s & READER_MASK) == READER_MASK {
                for _ in 0..backoff_spins(spins) {
                    core::hint::spin_loop();
                }
                spins += 1;
                continue;
            }

            // Strong CAS: a spurious failure here would just cost a retry, but
            // `false` success would be a correctness bug, so don't risk it.
            // Adding 1 only touches the low reader-count bits.
            if self
                .state
                .compare_exchange(s, s + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return RwLockReadGuard { lock: self };
            }
            spins = 0;
        }
    }

    /// Acquire the write lock. Blocks until no readers and no other writer.
    ///
    /// TODO: Implement write lock acquisition (writer-priority)
    /// 1. Set WRITER_WAITING first: fetch_or(WRITER_WAITING, Release) so new readers will block.
    /// 2. In a loop: load state; if any readers (READER_MASK) or WRITER_HOLDING, spin_loop and continue.
    /// 3. Try compare_exchange(WRITER_WAITING, WRITER_HOLDING, ...) to take the lock; or compare_exchange(0, WRITER_HOLDING, ...) if a writer just released.
    /// 4. On success return RwLockWriteGuard { lock: self }.
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        // Announce arrival before contending for ownership. From this instant on
        // readers see WRITER_WAITING and stop queueing up behind themselves, so
        // the reader stream cannot starve us. fetch_or keeps a bit another writer
        // already set, so the flag stays up as long as anyone is still waiting.
        //
        // The success ordering must be ACQUIRE, not just Release: this is also
        // where we learn about the previous critical sections. A pure Release
        // fetch_or would let our first state load run without any happens-before
        // edge to the last releaser's fetch_and/fetch_sub, so we could read a
        // stale WRITER_HOLDING bit, believe the lock is free, and enter while a
        // writer or reader is still inside. AcqRel orders every handoff through
        // this single location, which is what makes the CAS below a correct
        // mutual-exclusion claim.
        self.state.fetch_or(WRITER_WAITING, Ordering::AcqRel);

        let mut spins: u32 = 0;
        loop {
            // Acquire on every retry too: a relaxed load could keep returning a
            // stale value long after the owner cleared its bits.
            let s = self.state.load(Ordering::Acquire);
            if spins > 5_000_000 {
                eprintln!("WLOOP t={:?} s={:#010x} claims={} fast={} else={} spin0={}",
                    std::thread::current().id(), s,
                    CLAIMS.swap(0, Ordering::Relaxed), FASTC.swap(0, Ordering::Relaxed),
                    ELSEC.swap(0, Ordering::Relaxed), ZEROC.swap(0, Ordering::Relaxed));
                spins = 0;
            }

            // Fast path: nobody holds the lock and no other writer has claimed
            // the waiting slot, so install ourselves atomically. Clearing
            // WRITER_WAITING here is exactly what lets readers pile back in.
            if s == WRITER_WAITING {
                if self
                    .state
                    .compare_exchange(
                        WRITER_WAITING,
                        WRITER_HOLDING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    if CLAIMS.fetch_add(1, Ordering::Relaxed) < 12 { eprintln!("W-CASFAIL fast"); }
                } else {
                    return RwLockWriteGuard { lock: self };
                }
            } else if s & (WRITER_HOLDING | WRITER_WAITING) == WRITER_WAITING
                && (s & READER_MASK) == 0
            {
                // The lock body itself is free (no holder, zero readers), but at
                // least one other writer shares the waiting slot. Claim ownership
                // while KEEPING WRITER_WAITING set: as long as any writer is
                // still queued the flag must stay up, both to keep new readers
                // out and so the remaining waiters' expected value keeps matching.
                // Exactly one of the racing CASes succeeds; the losers respin.
                if self
                    .state
                    .compare_exchange(
                        s,
                        s | WRITER_HOLDING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return RwLockWriteGuard { lock: self };
                }
            }

            if spins > 3_000_000 { eprintln!("STUCK w s={:#010x} readers={} holding={} waiting={}", s, s & READER_MASK, s & WRITER_HOLDING != 0, s & WRITER_WAITING != 0); spins = 0; }
            let hints = backoff_spins(spins);
            for _ in 0..hints {
                core::hint::spin_loop();
            }
            spins += 1;
        }
    }
}

/// Guard for a read lock; releases the read lock on drop.
pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>,
}

// TODO: Implement Deref for RwLockReadGuard
// Return shared reference to data: unsafe { &*self.lock.data.get() }
impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: we hold a read-count slot, so no writer can be mutating the
        // data while this guard is alive; shared access is exactly what readers
        // are allowed.
        unsafe { &*self.lock.data.get() }
    }
}

// TODO: Implement Drop for RwLockReadGuard
// Decrement reader count: self.lock.state.fetch_sub(1, Ordering::Release)
impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        // Release: reads performed through the guard happen-before the next
        // writer acquires. Only the low reader-count bits are touched, so the
        // writer flags stay intact.
        self.lock.state.fetch_sub(1, Ordering::Release);
    }
}

/// Guard for a write lock; releases the write lock on drop.
pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>,
}

// TODO: Implement Deref for RwLockWriteGuard
// Return shared reference: unsafe { &*self.lock.data.get() }
impl<T> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the writer is exclusive, so aliasing rules are trivially met.
        unsafe { &*self.lock.data.get() }
    }
}

// TODO: Implement DerefMut for RwLockWriteGuard
// Return mutable reference: unsafe { &mut *self.lock.data.get() }
impl<T> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: WRITER_HOLDING means zero readers and zero other writers, and
        // `&mut self` keeps the guard itself unaliased.
        unsafe { &mut *self.lock.data.get() }
    }
}

// TODO: Implement Drop for RwLockWriteGuard
// Clear writer bits so lock is free: self.lock.state.fetch_and(!(WRITER_HOLDING | WRITER_WAITING), Ordering::Release)
impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        // Release publishes our mutations to whoever acquires next.
        //
        // Only WRITER_HOLDING is cleared. WRITER_WAITING must stay set whenever
        // some writer is still queued: waiting writers claim ownership with
        // compare_exchange(WRITER_WAITING, WRITER_HOLDING), so erasing the bit
        // here would leave their expected value unmatched forever and deadlock
        // the whole lock (the classic "lost wakeup" bug). Leaving it set also
        // keeps new readers out until a waiter actually runs, which is precisely
        // the writer-priority guarantee.
        //
        // The last waiter clears the flag itself when its CAS succeeds, so the
        // bit can never get stuck on; the reader-count field is untouched because
        // it is provably zero while a writer holds the lock.
        self.lock.state.fetch_and(!WRITER_HOLDING, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_multiple_readers() {
        let lock = Arc::new(RwLock::new(0u32));
        let mut handles = vec![];
        for _ in 0..10 {
            let l = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                let g = l.read();
                assert_eq!(*g, 0);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_writer_excludes_readers() {
        let lock = Arc::new(RwLock::new(0u32));
        let lock_w = Arc::clone(&lock);
        let writer = thread::spawn(move || {
            let mut g = lock_w.write();
            *g = 42;
        });
        writer.join().unwrap();
        let g = lock.read();
        assert_eq!(*g, 42);
    }

    #[test]
    fn test_concurrent_reads_after_write() {
        let lock = Arc::new(RwLock::new(Vec::<i32>::new()));
        {
            let mut g = lock.write();
            g.push(1);
            g.push(2);
        }
        let mut handles = vec![];
        for _ in 0..5 {
            let l = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                let g = l.read();
                assert_eq!(g.len(), 2);
                assert_eq!(&*g, &[1, 2]);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_writes_serialized() {
        let lock = Arc::new(RwLock::new(0u64));
        let mut handles = vec![];
        for _ in 0..10 {
            let l = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let mut g = l.write();
                    *g += 1;
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(*lock.read(), 1000);
    }
}
