use futures::{
    Future,
    FutureExt,
    task::Context,
    Poll,
    channel::oneshot,
};

use std::ops::{Deref, DerefMut};
use std::pin::Pin;

pub struct Monitored<T>(T, oneshot::Sender<()>);
pub struct FlatlineFuture(oneshot::Receiver<()>);

pub fn monitor<T>(obj: T) -> (Monitored<T>, FlatlineFuture) {
    let (sender, receiver) = oneshot::channel();
    (Monitored(obj, sender), FlatlineFuture(receiver))
}

impl<T> Deref for Monitored<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Monitored<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl Future for FlatlineFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match oneshot::Receiver::poll_unpin(&mut self.0, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => Poll::Ready(()),
        }
    }
}
