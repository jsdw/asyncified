use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Poll, Waker};

/// Create a naive but fast enough async->sync bounded channel.
/// - The sender is non-blocking and can operate in an async context.
/// - The receiver is blocking and expected to operate in a sync context.
pub fn new<T>(size: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Inner {
        size,
        waiter: Condvar::new(),
        next_sender_id: AtomicU64::new(1),
        senders: AtomicU64::new(1),
        locked: Mutex::new(Shared {
            items: Default::default(),
            wakers: Default::default(),
        }),
    });

    let sender = Sender {
        id: 0,
        inner: Arc::downgrade(&inner),
    };
    let receiver = Receiver {
        inner,
        items: Default::default(),
    };

    (sender, receiver)
}

pub enum SendError<T> {
    /// If we try to send an item, but the receiver is
    /// dropped before we can, we'll be given the item
    /// back in this error.
    ReceiverDropped(T),
    /// If we try polling the `send` future again after
    /// it's completed, we'll get this back.
    AlreadyReady,
}

pub struct Sender<T> {
    id: u64,
    inner: Weak<Inner<T>>,
}

impl<T: Send + 'static> Sender<T> {
    pub fn send(&self, item: T) -> impl Future<Output = Result<(), SendError<T>>> + 'static {
        let inner = self.inner.clone();
        let id = self.id;
        let mut maybe_item = Some(item);
        std::future::poll_fn(move |ctx| {
            // If there is no item in our option, we've already
            // finished, so return an error.
            let Some(item) = maybe_item.take() else {
                return Poll::Ready(Err(SendError::AlreadyReady));
            };

            // Try to upgrade Arc to a strong pointer to access
            // contents. If we can't, it means receiver was dropped,
            // so hand back the item as an error.
            let Some(inner) = inner.upgrade() else {
                return Poll::Ready(Err(SendError::ReceiverDropped(item)));
            };

            let mut locked = inner.locked.lock().unwrap();

            // Not ready yet if at capacity. We'll expect the
            // other side to wake the waker when it consumes items
            // so that we can try again. Put the item back into the
            // option for next time.
            if locked.items.len() >= inner.size {
                locked.wakers.insert(id, ctx.waker().clone());
                drop(locked);
                maybe_item = Some(item);
                return Poll::Pending;
            }

            // Else, push the item. Tell the
            // other side to stop waiting.
            locked.items.push_back(item);
            drop(locked);
            inner.waiter.notify_one();
            Poll::Ready(Ok(()))
        })
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let mut id = 0;

        // If the receiver still exists, make sure to
        // tell it about the new Sender, and give the sender
        // a unique ID.
        if let Some(inner) = self.inner.upgrade() {
            id = inner.next_sender_id.fetch_add(1, Ordering::Relaxed);
            inner.senders.fetch_add(1, Ordering::Relaxed);
        }

        Self {
            id,
            inner: self.inner.clone(),
        }
    }
}

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender")
            .field("id", &self.id)
            .field("inner", &"<inner>")
            .finish()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            let num_senders = inner.senders.fetch_sub(1, Ordering::Relaxed);

            // Remember; the _previous_ value was returned, so
            // inner.senders is now 0.
            if num_senders == 1 {
                inner.waiter.notify_one();
            }
        }
    }
}

pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
    items: VecDeque<T>,
}

impl<T> Receiver<T> {
    pub fn recv(&mut self) -> Option<T> {
        if self.items.is_empty() {
            let mut shared = self.inner.locked.lock().unwrap();
            let senders = &self.inner.senders;

            // Wait until notified that more items have been put in the queue.
            // this may wake up spuriously, so loop until items. Stop waiting
            // if no more senders.
            while shared.items.is_empty() && senders.load(Ordering::Relaxed) > 0 {
                shared = self.inner.waiter.wait(shared).unwrap();
            }

            // Move the items to our local buffer, and wake the waker so that
            // futures waiting to send are woken, too.
            self.items.append(&mut shared.items);

            let wakers_iter = shared.wakers.drain();
            for (_, waker) in wakers_iter {
                waker.wake();
            }
        }

        // We'll start returning None when we're closed
        // and we run out of items.
        self.items.pop_front()
    }
}

struct Inner<T> {
    size: usize,
    waiter: Condvar,
    next_sender_id: AtomicU64,
    senders: AtomicU64,
    locked: Mutex<Shared<T>>,
}

struct Shared<T> {
    wakers: HashMap<u64, Waker>,
    items: VecDeque<T>,
}
