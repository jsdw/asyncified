# Changelog

## 0.6.0

- Add `AsyncifiedBuilder` to allow more configuration.
- Add `on_close` option, to schedule something to run when the container is dropped.
- Fix bug whereby channel wasn't closing and test that closing works.

## 0.5.0

- Add `Asyncified::new_ok()` method for when the thing you're constructing doesn't need to potentially return an error.
- Improve the docs.

## 0.4.0

- `Asyncified::new()` and `Asyncified::new_using()` now accept a constructor function that returns a `Result`. If the result is an `Err(..)` then we give this back to the caller and do nothing else. The caller can now `.await` this result if they are interested in it, to handle any errors.

## 0.3.1

- `Clone` and `Debug` impls on `Asyncified<T>` no longer require `T` to impl them.

## 0.3

- `Asyncified::new()` now takes a function, not a value. This allows the value to be constructed on the other thread, in case this construction itself is slow/blocking.
- Added `Asyncified::new_using()` to allow configuration of the thread that the value will be constructed in.

## 0.2

- Initial release and tinkering.

