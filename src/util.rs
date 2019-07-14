use futures::{
    prelude::*,
    task::{Context, Poll},
    channel::oneshot,
    channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver},
};

use runtime::time::Delay;

use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
    time::Duration
};

///////////////////////////////////////////////////////////////////////////////////////

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

///////////////////////////////////////////////////////////////////////////////////////

pub struct Timer<T> {
    sender: UnboundedSender<T>,
    receiver: UnboundedReceiver<T>,
}

impl<T: Send + 'static> Timer<T> {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Timer{sender, receiver}
    }

    pub fn add_alarm(&self, delay_ms: u64, memo: T) {
        let sender = self.sender.clone();
        #[allow(unused)] {
            runtime::spawn(async move {
                let delay = Delay::new(Duration::from_millis(delay_ms));
                delay.await;
                sender.unbounded_send(memo).expect("Timer channel failed");
            });
        }
    }

    pub fn reset(&mut self) {
        let (sender, receiver) = unbounded();
        self.sender = sender;
        self.receiver = receiver;
    }
}

impl<T> Stream for Timer<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.receiver.poll_next_unpin(cx)
    }
}
