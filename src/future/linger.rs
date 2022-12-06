use std::pin::Pin;
use std::task::{Poll, Context};

use futures::future::{self, Either, Future, FutureExt};
use tokio::task::JoinHandle;

use crate::future::notify_drop;

/// Somewhere in between owning a `Future` directory and holding a `JoinHandle`.
/// Lingered futures are asynchronously driven to completion but are only kept
/// alive as long as the future returned from this function is kept alive.
pub fn linger<T: Send + 'static>(fut: impl Future<Output = T> + Send + 'static) -> Linger<T> {
    let (tx, rx) = notify_drop::channel();

    let fut = async move {
        let cancel = rx.map(Either::Left);

        let fut = fut.map(Either::Right);
        futures::pin_mut!(fut);

        let either = future::select(cancel, fut);

        either.await.factor_first().0
    };

    Linger {
        join: tokio::spawn(fut),
        _guard: tx,
    }
}

pub struct Linger<T> {
    join: JoinHandle<Either<(), T>>,
    _guard: notify_drop::DropGuard,
}

impl<T> Future for Linger<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let join = unsafe { self.map_unchecked_mut(|s| &mut s.join) };
        join.poll(cx).map(|result| {
            match result {
                Ok(Either::Left(())) => {
                    // this value is only produced when DropGuard is dropped
                    // and we are witness to it being alive here
                    unreachable!()
                }
                Ok(Either::Right(val)) => {
                    // future completed
                    val
                }
                Err(err) => {
                    // join error must be a panic unless this task was somehow
                    // asynchronously cancelled, which is also reason to panic
                    let panic = err.into_panic();
                    std::panic::resume_unwind(panic);
                }
            }
        })
    }
}

