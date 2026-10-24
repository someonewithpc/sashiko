+++
title = "C++"
paths = ["**.cpp", "**.cc", "**.cxx", "**.hpp", "**.hh", "**.hxx"]
symbols = ["std", "unique_ptr", "shared_ptr", "weak_ptr", "make_unique", "make_shared"]
+++
# C++

## Lifetimes

- A reference, pointer, iterator, `std::string_view` or `std::span` into a
  container dangles once the container reallocates or the element is
  erased. `std::vector::push_back`, `emplace_back`, `insert`, `resize` and
  `reserve` can reallocate; `erase` invalidates from the erased element on.
  Holding `auto& x = vec[i]` across code that can add to `vec` is a use after
  free.
- Erasing from a `std::vector`, `std::deque` or `std::string` inside a
  range-for over it is undefined behaviour. For `std::list`, `std::map`,
  `std::set` and the `unordered_*` containers, erase invalidates only the
  erased element, so erasing an element other than the current one is
  fine and erasing the current one is the bug. Use the iterator returned
  by `erase`, or `std::erase_if`.
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
- `std::move` leaves a standard library object in a valid but unspecified
  state. Reading its value afterwards is a bug, but calls without
  preconditions (`clear()`, `empty()`, `size()`, assignment) are fine.
  `std::unique_ptr` and `std::shared_ptr` are guaranteed null after a move,
  and a moved-from `std::weak_ptr` is empty (`expired()` is true, `lock()`
  returns an empty pointer), so checking one is correct.

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

## Undefined behaviour

- Signed integer overflow and shifts by a negative count or by the type's
  width or more are undefined. Before C++20, left-shifting a negative value
  is undefined and right-shifting one is implementation defined; since
  C++20 both are defined. Arithmetic on sizes and coordinates from outside
  needs range checks.
- Reading an inactive union member is undefined, except through the common
  initial sequence of standard-layout struct members. GCC and clang
  document union type punning as supported, so do not flag it in code that
  only builds with them. A `reinterpret_cast` is
  itself fine; accessing the object through a type the aliasing rules
  forbid is undefined, while access through `char`, `unsigned char` and
  `std::byte` is allowed. Writing through a pointer whose `const` was cast
  away is undefined only when the object was defined `const`. Use
  `std::memcpy` or `std::bit_cast` for type punning.
- Members of a struct without a constructor or default member initializers
  hold indeterminate values when the struct is default-initialized with
  automatic or dynamic storage (`T x;`, `new T`). `T x{}`, `T x = {}`,
  `new T()` and static storage zero-initialize them. A struct passed to a C
  API (an ioctl argument, a DRM property blob) must be zero-initialized.
- `static_cast` to a derived type that the object is not is undefined.
  `dynamic_cast` on a pointer returns null instead; on a reference it
  throws `std::bad_cast`.

## Exceptions and C interfaces

- An exception that propagates out of a callback called from C code (GLib,
  Wayland, libdrm, libinput) is not defined by the standard; with GCC and
  clang it usually ends in `std::terminate`. Callbacks registered with C libraries must catch everything.
- `std::stoi`, `std::stoul` and similar throw on bad input. Parsing config or
  protocol data with them needs a `try` or `std::from_chars`.
- A destructor that throws during stack unwinding calls `std::terminate`.
