use futures::future::{Future, FutureExt};
use tokio::sync::oneshot;

pub fn channel() -> (DropGuard, impl Future<Output = ()>) {
    // we use a oneshot as the underlying mechanism because the receiver side
    // completes with a RecvError when the sender drops before sending
    let (tx, rx) = oneshot::channel();

    (DropGuard { _tx: tx }, rx.map(|_| ()))
}

pub struct DropGuard {
    _tx: oneshot::Sender<()>,
}
