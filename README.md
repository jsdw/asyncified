# Asyncified

A small, zero dependency, runtime agnostic Rust library for taking something that needs to operate in a single blocking thread and making it available to be operated on from an multiple tasks in an async context.

```rust
use asyncified::Asyncified;

// We can construct some blocking thing in a new thread:
let async_conn = Asyncified::new(|| database::connection());

// And then we can run blocking code which is handed a mutable reference
// to this thing, and await the result in a non-blocking way from our
// async context:
let res = async_conn.call(|db_conn| {
    let res = db_conn.execute("SELECT * FROM foo");
    res
}).await;
```

Calling `Asyncified::new()` spawns a new thread for the value to live in. That thread remains active until all of the `Asyncified` instance(s) are dropped. Using `.call()` dispatches functions to this new thread to be run in sequence in a blocking context.

The value passed to `Asyncified::new()` must be `Send + 'static` to be sent to this new thread, but does not need to be `Sync`, as it's only ever accessed on this one thread.

**Warning:** The channel implementations used to send functions and receive results from the sync thread are naive. In quick tests, the time taken to update a value and return it is in the order of a few microseconds, so it's good enough for my needs at the moment, since it's expected to be used with things like database connections which will dwarf this overhead.