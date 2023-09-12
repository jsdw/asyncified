use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

/// Create a naive but fast enough sync->async oneshot channel.
/// - The sender is blocking and expected to operate in a sync context.
/// - The receiver is non-blocking and operates in an async context.
pub fn new<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(Inner {
        item: None,
        waker: None,
    }));

    let sender = Sender {
        inner: inner.clone(),
    };
    let receiver = Receiver { inner };

    (sender, receiver)
}

pub struct Sender<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T> Sender<T> {
    // Send from a sync thread, waking the async
    // receiver if it's waiting already.
    pub fn send(self, item: T) {
        let mut inner = self.inner.lock().unwrap();
        inner.item = Some(item);
        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T: Send + 'static> Receiver<T> {
    /// Receive the value in an async context.
    pub fn recv(self) -> impl Future<Output = T> {
        std::future::poll_fn(move |context| {
            let mut inner = self.inner.lock().unwrap();

            match inner.item.take() {
                Some(item) => Poll::Ready(item),
                None => {
                    inner.waker = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}

struct Inner<T> {
    item: Option<T>,
    waker: Option<Waker>,
}
