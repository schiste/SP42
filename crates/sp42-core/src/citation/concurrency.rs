//! Bounded-concurrency map — the one worker-pool primitive (ADR-0006 §4).
//!
//! [`map_with_concurrency`] runs an async operation over each input with at most
//! `limit` in flight at once, returning the results in **input order**. The panel
//! fan-out in [`crate::citation::verify`] uses it to call each model concurrently while
//! capping load on the inference endpoint.

use std::future::Future;

use futures::stream::{self, StreamExt};

/// Map `f` over `items` with at most `limit` concurrent in-flight futures, returning
/// results in input order.
///
/// `limit` is floored to `1` (a value of `0` runs sequentially). An empty input yields
/// an empty output without ever calling `f`.
pub async fn map_with_concurrency<T, R, Fut, F>(
    items: impl IntoIterator<Item = T>,
    limit: usize,
    f: F,
) -> Vec<R>
where
    F: Fn(T, usize) -> Fut,
    Fut: Future<Output = R>,
{
    let effective = limit.max(1);
    stream::iter(items.into_iter().enumerate())
        .map(move |(index, item)| f(item, index))
        .buffered(effective)
        .collect::<Vec<R>>()
        .await
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use futures::executor::block_on;

    use super::map_with_concurrency;

    #[test]
    fn results_are_in_input_order() {
        let result = block_on(map_with_concurrency(
            vec![10, 20, 30],
            2,
            |item, index| async move { (index, item * item) },
        ));
        assert_eq!(result, vec![(0, 100), (1, 400), (2, 900)]);
    }

    #[test]
    fn empty_input_never_calls_f() {
        let calls = AtomicUsize::new(0);
        let result: Vec<()> = block_on(map_with_concurrency(Vec::<u32>::new(), 4, |_item, _i| {
            calls.fetch_add(1, Ordering::SeqCst);
            async {}
        }));
        assert!(result.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn zero_limit_is_floored_to_one() {
        let result = block_on(map_with_concurrency(
            vec![1, 2, 3],
            0,
            |item, _i| async move { item + 1 },
        ));
        assert_eq!(result, vec![2, 3, 4]);
    }

    /// A future that returns `Pending` once (waking itself) before resolving, so several
    /// can be observed in flight simultaneously under a single-threaded executor.
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    #[test]
    fn concurrency_never_exceeds_the_limit() {
        let active = Mutex::new(0usize);
        let max_seen = Mutex::new(0usize);
        let result = block_on(map_with_concurrency(vec![(); 6], 2, |(), index| {
            let active = &active;
            let max_seen = &max_seen;
            async move {
                {
                    let mut a = active.lock().expect("lock");
                    *a += 1;
                    let mut m = max_seen.lock().expect("lock");
                    *m = (*m).max(*a);
                }
                YieldOnce(false).await;
                {
                    let mut a = active.lock().expect("lock");
                    *a -= 1;
                }
                index
            }
        }));
        assert_eq!(result, vec![0, 1, 2, 3, 4, 5]);
        assert!(
            *max_seen.lock().expect("lock") <= 2,
            "peak concurrency exceeded the limit"
        );
        assert_eq!(*active.lock().expect("lock"), 0);
    }
}
