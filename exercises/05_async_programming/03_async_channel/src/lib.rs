//! # Async Channel
//!
//! In this exercise, you will use `tokio::sync::mpsc` async channels to implement producer-consumer pattern.
//!
//! ## Concepts
//! - `tokio::sync::mpsc::channel` creates bounded async channels
//! - Async `send` and `recv`
//! - Channel closing mechanism (receiver returns None after all senders are dropped)

use tokio::sync::mpsc;

/// Async producer-consumer:
/// - Create a producer task that sends each element from items sequentially
/// - Create a consumer task that receives all elements and collects them into Vec for return
///
/// Hint: Set channel capacity to items.len().max(1)
pub async fn producer_consumer(items: Vec<String>) -> Vec<String> {
    // Capacity >= items.len() lets the producer finish without ever awaiting on a full
    // channel; an empty input still needs capacity 1, since tokio rejects capacity 0.
    let (tx, mut rx) = mpsc::channel::<String>(items.len().max(1));

    tokio::spawn(async move {
        for item in items {
            // Fails only if the consumer was dropped; nothing to do in that case.
            if tx.send(item).await.is_err() {
                break;
            }
        }
        // `tx` drops here, closing the channel so the consumer's recv() returns None.
    });

    let consumer = tokio::spawn(async move {
        let mut received = Vec::new();
        while let Some(item) = rx.recv().await {
            received.push(item);
        }
        received
    });

    consumer.await.expect("consumer task panicked")
}

/// Fan‑in pattern: multiple producers, one consumer.
/// Create `n_producers` producers, each sending `"producer {id}: message"`.
/// Consumer collects all messages, sorts them, and returns.
pub async fn fan_in(n_producers: usize) -> Vec<String> {
    let (tx, mut rx) = mpsc::channel::<String>(n_producers.max(1));

    // Fan-in: every producer holds its own clone of the sender.
    for id in 0..n_producers {
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(format!("producer {}: message", id)).await;
        });
    }
    // The original sender must go, or the channel never closes and recv() waits forever.
    drop(tx);

    let consumer = tokio::spawn(async move {
        let mut messages = Vec::new();
        while let Some(msg) = rx.recv().await {
            messages.push(msg);
        }
        // Arrival order is nondeterministic, so sort to get a stable result.
        messages.sort();
        messages
    });

    consumer.await.expect("consumer task panicked")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_producer_consumer() {
        let items = vec!["hello".into(), "async".into(), "world".into()];
        let result = producer_consumer(items.clone()).await;
        assert_eq!(result, items);
    }

    #[tokio::test]
    async fn test_producer_consumer_empty() {
        let result = producer_consumer(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fan_in() {
        let result = fan_in(3).await;
        assert_eq!(
            result,
            vec![
                "producer 0: message",
                "producer 1: message",
                "producer 2: message",
            ]
        );
    }

    #[tokio::test]
    async fn test_fan_in_single() {
        let result = fan_in(1).await;
        assert_eq!(result, vec!["producer 0: message"]);
    }
}
