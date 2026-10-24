# Concurrency and event loops

Most userspace C and C++ desktop code is single threaded around an event
loop. Its concurrency bugs are about order and reentrancy more than locks.
Code that does use threads has the usual data races on top.

## Event loop order

- A callback runs later, after other callbacks. Anything checked when the
  callback was scheduled (an object exists, a device is connected, a
  configuration value) can be different when it runs. Check again in the
  callback.
- An emitted signal, hook or listener runs arbitrary code before `emit`
  returns. That code can free the emitter, remove listeners, change the
  container being iterated or re-enter the function that emitted. Code after
  `emit` must not assume the state from before it.
- Nested main loop iterations (`g_main_context_iteration`, a synchronous
  D-Bus call with a nested loop, a modal dialog) run unrelated callbacks in
  the middle of a function.
- Two async operations started together can complete in either order. Code
  that assumes the first reply comes first breaks when the system is loaded.

## Threads

- Data shared between threads needs a lock, an atomic or a hand-off through a
  queue. A plain `gboolean` or `bool` flag written in one thread and read in
  another is a data race.
- GObject reference counting is thread safe; most other GObject and GTK state
  is not. Touching a widget, a non-thread-safe GObject or a `GHashTable` from
  a worker thread is a bug unless the project documents otherwise.
- A thread must be joined or detached. An object freed while its thread
  still runs is a use after free in that thread.
- Locks must be taken in one order everywhere. Calling out to a callback,
  signal or another component while holding a lock invites a deadlock when
  that code takes the same lock or one ordered before it.
- `std::mutex` locked in one thread and unlocked in another is undefined.
  `GMutex` has the same rule.

## POSIX signals

- A signal handler may only call async-signal-safe functions. `malloc`,
  `printf`, `g_*` logging and most GLib calls are not. Use
  `g_unix_signal_add` (GLib) or a self-pipe or signalfd to handle signals in
  the main loop.
- `SIGPIPE` kills a process that writes to a closed socket or pipe unless it
  is ignored or `MSG_NOSIGNAL` is used.

## Processes

- A child process must be reaped (`waitpid`, `g_child_watch_add`) or it stays
  a zombie.
- Arguments for a child process belong in an argv array. Building a shell
  command string from data that includes file names, device names or user
  input is a command injection.
