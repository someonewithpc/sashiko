# Memory and ownership

## Follow every allocation to its free

For each allocation, reference, file descriptor, mapping or registration the
patch adds or moves, find who owns it and where it is released, on every
path, including failure and teardown. Then check that nothing uses it after
that point.

- Ownership moves at API boundaries. GObject introspection annotations
  (`(transfer full)`, `(transfer none)`, `(transfer container)`) and the
  function's documentation say who frees. Read the callee before assuming.
- A pointer stored in a long-lived structure must stay valid as long as the
  structure can reach it. A borrowed pointer stored there dangles when its
  real owner frees it.
- An object freed while something else still points at it (a list, a hash
  table, a callback's user data, a pending async operation, a timer) is a use
  after free waiting for that something to run.
- Freeing twice usually comes from two owners: a cleanup attribute and an
  explicit free, a caller and a callee that both think they own the value, or
  dispose that frees a field without clearing it.

## File descriptors and other handles

- Every `open`, `socket`, `dup`, `pipe`, `eventfd`, `timerfd`, `memfd_create`
  and fd received over a socket or D-Bus needs a matching `close` on every
  path. A descriptor handed to another object (a `GUnixFDList`, a
  `GInputStream` created with `close_fd = TRUE`, a DRM lease) is closed by
  that object; closing it again closes whatever reused the number.
- Descriptors that must not leak into child processes need `O_CLOEXEC` or
  `FD_CLOEXEC`.
- `mmap` needs `munmap` with the same length.

## Reference counts

- Every `ref` needs an `unref` on every path. Taking a reference "for the
  callback" and dropping it only on success leaks it on failure and
  cancellation.
- Dropping the last reference runs teardown immediately. Code that unrefs and
  then reads a field of the same object reads freed memory.
- An object that unrefs itself from a callback must not touch its own fields
  after that call.

## Teardown order

- Stop everything that can call into an object before freeing it: remove
  sources, disconnect signals, cancel async operations, unregister D-Bus
  objects, remove listeners and hooks, join or stop threads.
- Free children before the parent they point back to only if the children do
  not call into the parent while being freed.
- On device unplug or plugin unload, pending callbacks for the device or
  plugin still arrive. Each one must find either a live object or a cleared
  pointer, never freed memory.
