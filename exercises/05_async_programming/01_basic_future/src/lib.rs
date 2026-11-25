//! # Manual Future Implementation
//!
//! In this exercise, you will manually implement the `Future` trait for custom types to understand the core mechanism of asynchronous runtime.
//!
//! ## Concepts
//! - `std::future::Future` trait
//! - `Poll::Ready` and `Poll::Pending`
//! - The role of `Waker`: notifying the runtime to poll again

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Countdown Future: decrements count by 1 each time it's polled,
/// returns `"liftoff!"` when count reaches 0.
pub struct CountDown {
    pub count: u32,
}

impl CountDown {
    pub fn new(count: u32) -> Self {
        Self { count }
    }
}

// TODO: Implement Future trait for CountDown
// - Output type is &'static str
// - Each poll: if count == 0, return Poll::Ready("liftoff!")
// - Otherwise count -= 1, call cx.waker().wake_by_ref(), return Poll::Pending
//
// Hint: Use `self.get_mut()` to get `&mut Self` (since self is Pin<&mut Self>)
impl Future for CountDown {
    type Output = &'static str;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // `CountDown` holds no self-references, so getting `&mut Self` out of the Pin
        // is sound.
        let this = self.get_mut();
        if this.count == 0 {
            return Poll::Ready("liftoff!");
        }
        // Returning Pending must be paired with a wake-up, otherwise the task would
        // never be polled again and the runtime would park forever.
        this.count -= 1;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Yield-only-once Future: first poll returns Pending, second returns Ready(()).
/// This is the minimal example of an asynchronous state machine.
pub struct YieldOnce {
    yielded: bool,
}

impl YieldOnce {
    pub fn new() -> Self {
        Self { yielded: false }
    }
}

// TODO: Implement Future trait for YieldOnce
// - Output type is ()
// - First poll: set yielded = true, wake waker, return Pending
// - Second poll: return Ready(())
impl Future for YieldOnce {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.yielded {
            // Second poll: the single yield already happened, so we are done.
            return Poll::Ready(());
        }
        // First poll: remember it, schedule another poll, and report Pending - exactly
        // what an async state machine does between two await points.
        this.yielded = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_countdown_zero() {
        let result = CountDown::new(0).await;
        assert_eq!(result, "liftoff!");
    }

    #[tokio::test]
    async fn test_countdown_three() {
        let result = CountDown::new(3).await;
        assert_eq!(result, "liftoff!");
    }

    #[tokio::test]
    async fn test_yield_once() {
        YieldOnce::new().await;
    }

    #[tokio::test]
    async fn test_countdown_large() {
        let result = CountDown::new(100).await;
        assert_eq!(result, "liftoff!");
    }
}
