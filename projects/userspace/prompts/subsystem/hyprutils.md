# Hyprland and hyprutils

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
- When a plugin unloads, Hyprland removes the function hooks, dispatchers,
  layouts, decorations, hyprctl commands and config values it registered
  through `HyprlandAPI` with its handle. The plugin must undo what the host
  does not track: event callbacks and signal listeners it holds, timers and
  event sources, and callbacks or objects handed to compositor code outside
  the API. Code left pointing into the unloaded plugin's memory crashes the
  compositor at the next call.
- Compositor objects (windows, workspaces, monitors) can disappear between
  frames. A raw pointer or reference kept across event loop iterations
  dangles; keep a `WP` and check it.
