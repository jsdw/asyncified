//! # Asyncified
//!
//! This small, zero-dependency library provides a wrapper
//! to hide synchronous, blocking types behind an async
//! interface that is runtime agnostic.
//!
//! # Example
//!
//! ```rust
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # struct SlowThing;
//! # impl SlowThing {
//! #     fn new() -> SlowThing {
//! #         std::thread::sleep(std::time::Duration::from_secs(1));
//! #         SlowThing
//! #     }
//! # }
//! use asyncified::Asyncified;
//!
//! // Construct a thing (that could take time) inside
//! // the Asyncified container, awaiting it to be ready.
//! // prefer `new()` if you want to be able to return an error.
//! let s = Asyncified::builder().build_ok(SlowThing::new).await;
//!
//! // Perform some potentially slow operation on the thing
//! // inside the container, awaiting the result.
//! let n = s.call(|slow_thing| {
//!     std::thread::sleep(std::time::Duration::from_secs(1));
//!     123usize
//! }).await;
//! # assert_eq!(n, 123);
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod channel;
mod oneshot;

type Func<T> = Box<dyn FnOnce(&mut T) + Send + 'static>;

pub struct AsyncifiedBuilder<T> {
    channel_size: usize,
    on_close: Option<Func<T>>,
    thread_builder: Option<std::thread::Builder>,
}

impl<T: 'static> Default for AsyncifiedBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> AsyncifiedBuilder<T> {
    /// Construct a new builder.
    pub fn new() -> Self {
        Self {
            channel_size: 16,
            on_close: None,
            thread_builder: None,
        }
    }

    /// How many items can be queued at a time to be executed on
    /// the Asyncified container.
    pub fn channel_size(mut self, size: usize) -> Self {
        self.channel_size = size;
        self
    }

    /// Configure the thread that the asyncified item will live on.
    pub fn thread_builder(mut self, builder: std::thread::Builder) -> Self {
        self.thread_builder = Some(builder);
        self
    }

    /// Configure a single function to run when the [`Asyncified`]
    /// container is dropped.
    pub fn on_close<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut T) + Send + 'static,
    {
        self.on_close = Some(Box::new(f));
        self
    }

    /// This is a shorthand for [`AsyncifiedBuilder::build()`] for when the thing
    /// you're constructing can't fail.
    pub async fn build_ok<F>(self, val_fn: F) -> Asyncified<T>
    where
        F: Send + 'static + FnOnce() -> T,
    {
        self.build(move || Ok::<_, ()>(val_fn()))
            .await
            .expect("function can't fail")
    }

    /// This is passed a constructor function, and constructs the resulting
    /// value on a new thread, returning any error in doing so. Use
    /// [`Asyncified::call()`] to perform operations on the constructed value.
    ///
    /// It's also passed a thread builder, giving you control over the thread
    /// that the value is spawned into.
    pub async fn build<E, F>(self, val_fn: F) -> Result<Asyncified<T>, E>
    where
        E: Send + 'static,
        F: Send + 'static + FnOnce() -> Result<T, E>,
    {
        let thread_builder = self
            .thread_builder
            .unwrap_or_else(|| std::thread::Builder::new().name("Asyncified thread".to_string()));

        // The default size of the bounded channel, 16, is somewhat
        // arbitrarily chosen just to help reduce locking on
        // reads from the channel when there are many writes
        // (ie it can lock once per 16 reads). `call` will
        // always wait until a result anyway.
        let channel_size = self.channel_size;

        let (tx, mut rx) = channel::new::<Func<T>>(channel_size);
        let (res_tx, res_rx) = oneshot::new::<Result<(), E>>();

        // This runs when the task is going to end (eg no more
        // open channels).
        let on_close = self.on_close;

        // As long as there are senders, we'll
        // receive and run functions in the separate thread.
        thread_builder
            .spawn(move || {
                let mut val = match val_fn() {
                    Ok(val) => {
                        res_tx.send(Ok(()));
                        val
                    }
                    Err(e) => {
                        res_tx.send(Err(e));
                        return;
                    }
                };
                while let Some(f) = rx.recv() {
                    f(&mut val)
                }
                if let Some(on_close) = on_close {
                    on_close(&mut val)
                }
            })
            .expect("should be able to spawn new thread for Asyncified instance");

        res_rx.recv().await?;
        Ok(Asyncified { tx })
    }
}

/// The whole point.
pub struct Asyncified<T> {
    tx: channel::Sender<Func<T>>,
}

impl<T> Clone for Asyncified<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<T> std::fmt::Debug for Asyncified<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asyncified")
            .field("tx", &"<channel::Sender>")
            .finish()
    }
}

impl<T: 'static> Asyncified<T> {
    /// Construct a new [`Asyncified`] container with default values.
    /// Use [`Asyncified::builder()`] for more options.
    pub async fn new<E, F>(val_fn: F) -> Result<Asyncified<T>, E>
    where
        E: Send + 'static,
        F: Send + 'static + FnOnce() -> Result<T, E>,
    {
        Self::builder().build(val_fn).await
    }

    /// Configure a new [`Asyncified`] instance.
    pub fn builder() -> AsyncifiedBuilder<T> {
        AsyncifiedBuilder::new()
    }

    /// Execute the provided function on the thread that the asyncified
    /// value was moved onto, handing back the result when done. The async
    /// task that this is called from will be yielded while we're waiting
    /// for the call to finish.
    pub async fn call<R: Send + 'static, F: (FnOnce(&mut T) -> R) + Send + 'static>(
        &self,
        f: F,
    ) -> R {
        let (tx, rx) = oneshot::new::<R>();

        // Ignore any error, since we expect the thread to last until this
        // struct is dropped, and so sending should never fail.
        let _ = self
            .tx
            .send(Box::new(move |item| {
                let res = f(item);
                // Send res back via sync->async oneshot.
                tx.send(res);
            }))
            .await;

        rx.recv().await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn new_doesnt_block() {
        let start = Instant::now();

        // don't block if constructing the value takes a while (we can `await` to return only
        // when the value has been built).
        let _fut = Asyncified::new(|| {
            std::thread::sleep(Duration::from_secs(10));
            Ok::<_, ()>(())
        });

        // Should take <100ms to get here, not 10s.
        let d = Instant::now().duration_since(start).as_millis();
        assert!(d < 100);
    }

    #[tokio::test]
    async fn call_doesnt_block() {
        let a = Asyncified::new(|| Ok::<_, ()>(())).await.unwrap();

        let start = Instant::now();

        // The function takes 10s to complete:
        let _fut = a.call(|_| {
            std::thread::sleep(Duration::from_secs(10));
        });

        // But we just get a future back which doesn't block:
        let d = Instant::now().duration_since(start).as_millis();
        assert!(d < 100);
    }

    #[tokio::test]
    async fn basic_updating_works() {
        let a = Asyncified::new(|| Ok::<_, ()>(0u64)).await.unwrap();

        for i in 1..100_000 {
            assert_eq!(
                a.call(|n| {
                    *n += 1;
                    *n
                })
                .await,
                i
            );
        }
    }

    #[tokio::test]
    async fn parallel_updating_works() {
        let a = Asyncified::new(|| Ok::<_, ()>(0u64)).await.unwrap();

        // spawn 10 tasks which all increment the number
        let handles: Vec<_> = (0..10)
            .map({
                let a = a.clone();
                move |_| {
                    let a = a.clone();
                    tokio::spawn(async move {
                        for _ in 0..10_000 {
                            a.call(|n| {
                                *n += 1;
                                *n
                            })
                            .await;
                        }
                    })
                }
            })
            .collect();

        // wait for them all to finish
        for handle in handles {
            let _ = handle.await;
        }

        // the number should have been incremeted 100k times.
        assert_eq!(a.call(|n| *n).await, 100_000);
    }

    #[tokio::test]
    async fn on_close_is_called() {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let a = Asyncified::builder()
            .on_close(move |val| {
                let _ = tx.send(*val);
            })
            .build_ok(|| 0u64)
            .await;

        a.call(|v| *v = 100).await;

        drop(a);

        // The on_close should have set b to the value:
        assert_eq!(rx.await.unwrap(), 100);
    }
}
