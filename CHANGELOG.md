# Changelog

## 0.3.1

- `Clone` and `Debug` impls on `Asyncified<T>` no longer require `T` to impl them.

## 0.3

- `Asyncified::new()` now takes a function, not a value. This allows the value to be constructed on the other thread, in case this construction itself is slow/blocking.
- Added `Asyncified::new_using()` to allow configuration of the thread that the value will be constructed in.

## 0.2

- Initial release and tinkering.

