# GIO, main loop and D-Bus

## Main contexts and sources

- `g_idle_add`, `g_timeout_add` and `g_timeout_add_seconds` attach to the
  global default main context. Code running in a thread with its own
  `GMainContext` (pushed with `g_main_context_push_thread_default`) that uses
  them runs the callback in the wrong thread. Use `g_source_attach` on the
  intended context or `g_main_context_invoke`.
- A source callback returns `G_SOURCE_CONTINUE` to repeat or
  `G_SOURCE_REMOVE` to stop. Returning CONTINUE after freeing the data it
  uses fires again on freed memory. Calling `g_source_remove` on an ID whose
  callback already returned REMOVE warns and can remove an unrelated source
  that reused the ID; clear the stored ID in the callback when it returns
  REMOVE.
- The data passed to a source must outlive it. Use the `_full` variants with
  a `GDestroyNotify` when ownership moves to the source.

## Async operations

- An `_async` call takes a `GCancellable`, a callback and user data. The
  callback always runs, also after cancellation, with the error
  `G_IO_ERROR_CANCELLED` from `_finish`. A callback that touches `user_data`
  as an object assumes the object still exists. If the object can be freed
  before the reply, the caller must hold a reference in the user data,
  cancel in dispose and check for `G_IO_ERROR_CANCELLED` before touching
  the object, or both.
- Every `_async` call is completed by exactly one call to the matching
  `_finish` in its callback. Skipping `_finish` hides errors, and a returned
  object or string the caller should have taken is never freed.
- `GTask`: a task must be returned exactly once (`g_task_return_*`). Returning
  twice or never is a bug. The task holds a reference to its source object
  until it completes. `g_task_run_in_thread` runs the work function in a
  thread pool thread; anything it touches must be safe there.
- `g_cancellable_cancel` does not stop work already running; the operation
  notices at its next check. Freeing state right after cancelling, without
  waiting for the callback, is a use after free.

## GDBus server side

- A method handler receives a `GDBusMethodInvocation` and must answer it
  exactly once with `g_dbus_method_invocation_return_value`,
  `_return_error*` or `_return_gerror`. Those calls take ownership of the
  invocation. Not answering leaves the caller waiting until its timeout, and
  answering twice uses a freed invocation.
- `g_dbus_method_invocation_return_value` takes a tuple. Returning a single
  value needs `g_variant_new ("(s)", ...)`, not `g_variant_new_string`.
- Parameters come from another process. Check types and ranges before use;
  `g_variant_get` with a format that does not match aborts or logs a
  critical. Strings from D-Bus are valid UTF-8 but can be empty or very
  long.
- Authorization: a privileged method must check the caller, usually through
  polkit with the sender's bus name taken from the invocation. A check done
  against a cached PID or done after the action does not protect anything.
- Properties exposed on the bus need a change signal (`PropertiesChanged`)
  when they change, or clients keep stale values.
- `g_bus_own_name` callbacks: `name_lost` can fire at any time, including
  before `name_acquired`. Code that assumes it owns the name after the call
  returns races other instances.

## GDBus client side

- `g_dbus_proxy_new*` and generated `_proxy_new` functions can succeed while
  the remote service is absent; `g_dbus_proxy_get_name_owner` is NULL then.
- Synchronous calls (`g_dbus_proxy_call_sync`, `g_dbus_connection_call_sync`)
  block the calling thread and run no main loop iteration. In the main thread
  of a daemon or UI they stall everything until the timeout, which defaults to
  25 seconds.
- Signal subscriptions must be removed (`g_dbus_connection_signal_unsubscribe`)
  when the subscriber goes away.

## Files and streams

- `g_file_get_contents` returns the length separately; the data is
  NUL-terminated, but binary data can contain NUL bytes earlier.
- `g_input_stream_read` can return fewer bytes than requested. Use
  `g_input_stream_read_all` when the full amount is needed, and handle a
  short read at end of file.
- `g_file_replace_contents` and `g_file_set_contents` write atomically
  through a temporary file. Plain `fopen`/`fwrite` on a config file can leave
  it truncated on crash or full disk.

## GSettings

- `g_settings_new` with a schema that is not installed aborts the process. A
  library or optional feature should check with
  `g_settings_schema_source_lookup`.
