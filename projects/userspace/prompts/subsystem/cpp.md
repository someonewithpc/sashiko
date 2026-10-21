# C++

## Lifetimes

- A reference, pointer, iterator, `std::string_view` or `std::span` into a
  container dangles once the container reallocates or the element is
  erased. `std::vector::push_back`, `emplace_back`, `insert`, `resize` and
  `reserve` can reallocate; `erase` invalidates from the erased element on.
  Holding `auto& x = vec[i]` across code that can add to `vec` is a use after
  free.
- Erasing from a container inside a range-for over it is undefined
  behaviour. Use the iterator returned by `erase`, or `std::erase_if`.
- A callback that calls back into code which modifies the container being
  iterated (an emitted signal whose handler removes itself or another
  listener, a hook that closes a window) has the same problem one level
  removed.
- `std::string_view` or `const char*` taken from a temporary `std::string`
  (`std::string_view v = getName();` where `getName` returns by value,
  `s.c_str()` of a temporary) dangles at the end of the full expression.
- A lambda that captures `this` or a reference (`[&]`, `[this]`) and is stored
  in a callback, timer or signal outlives the scope that created it unless
  something removes it first. When the object is destroyed, the next call is
  a use after free. Capture a weak pointer and check it, or make sure the
  stored callback dies with the object.
- `std::move` leaves the source in a valid but unspecified state. Using the
  moved-from object's value afterwards (other than assigning to it or
  destroying it) is a bug.

## Ownership

- `std::unique_ptr` has one owner. `.get()` returns a borrowed pointer that
  dangles when the owner resets or is destroyed. `.release()` gives up
  ownership; the result must be freed by someone.
- `std::shared_ptr` cycles leak. Back pointers, parents and observers
  normally use `std::weak_ptr`.
- `std::weak_ptr::lock()` can return null. Using the result without checking
  is a null dereference. Checking `expired()` and then calling `lock()` races
  with another thread dropping the last reference; use the result of `lock()`.
- Constructing two `shared_ptr` from the same raw pointer creates two control
  blocks and frees the object twice. `shared_from_this()` in a constructor,
  or on an object not owned by a `shared_ptr`, throws `std::bad_weak_ptr` or
  is undefined, depending on the standard version.
- Raw `new` without a matching `delete` on every path, or `delete` on memory
  from `malloc`, a C library or `new[]`, is a bug.

## Hyprland and hyprutils

- `SP<T>` and `WP<T>` are `Hyprutils::Memory::CSharedPointer` and
  `CWeakPointer`, created with `makeShared<T>(...)`. Constructing a
  `CSharedPointer` from a raw pointer can duplicate ownership; the header
  says to prefer `makeShared`.
- `WP<T>::lock()` returns an empty `SP` when the object is gone; `expired()`
  can also be true while the object is still being destroyed. Check before
  dereferencing.
- `CSignalT::listen(...)` returns a `CHyprSignalListener` marked
  `[[nodiscard]]`: the listener is unregistered when that pointer is
  dropped. Storing it in a member keeps the handler alive for the member's
  lifetime; a handler that captures `this` must be stored in a member of
  `this` or removed before `this` is destroyed. `listenStatic` handlers live
  as long as the signal.
- A plugin must undo everything it registered (hooks, listeners, dispatchers,
  config values, layouts, render passes) when it is unloaded. Code left
  pointing into the unloaded plugin's memory crashes the compositor at the
  next call.
- Compositor objects (windows, workspaces, monitors) can disappear between
  frames. A raw pointer or reference kept across event loop iterations
  dangles; keep a `WP` and check it.

## Undefined behaviour

- Signed integer overflow, shifts by the type's width or more, and shifts of
  negative values are undefined. Arithmetic on sizes and coordinates from
  outside needs range checks.
- Reading an inactive union member, `reinterpret_cast` between unrelated types
  and casting away `const` to write are undefined or implementation
  defined. Use `std::memcpy` or `std::bit_cast` for type punning.
- Uninitialized members of a struct without a constructor or default member
  initializers hold garbage. A struct passed to a C API (an ioctl argument, a
  DRM property blob) must be zero-initialized.
- `static_cast` to a derived type that the object is not is undefined;
  `dynamic_cast` returns null instead.

## Exceptions and C interfaces

- An exception that propagates out of a callback called from C code (GLib,
  Wayland, libdrm, libinput) is undefined behaviour or terminates the
  program. Callbacks registered with C libraries must catch everything.
- `std::stoi`, `std::stoul` and similar throw on bad input. Parsing config or
  protocol data with them needs a `try` or `std::from_chars`.
- A destructor that throws during stack unwinding calls `std::terminate`.
