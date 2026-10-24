# libwayland and wlroots

## Listeners

- A `wl_listener` added with `wl_signal_add` is linked into the signal's
  list through `listener->link`. Freeing the struct that holds it without
  `wl_list_remove (&listener->link)` leaves a dangling link, and the next
  emit or list change writes freed memory.
- When an object's destroy signal fires, every listener on that object's
  signals, including the destroy listener itself, must be removed in that
  handler. The signals go away with the object.
- `wl_signal_emit` tolerates a handler removing its own listener, but not one
  that removes the next listener in the list. libwayland 1.22 added
  `wl_signal_emit_mutable` for signals whose handlers can remove other
  listeners.
- `wl_list_for_each` breaks if the loop body removes the current element; use
  `wl_list_for_each_safe`.

## Resources

- A client can destroy a resource, or disconnect, at any time. The destroy
  callback set with `wl_resource_set_implementation` then runs, and it must
  drop every pointer the compositor kept to the resource.
- When the compositor object behind a resource goes away first, the resource
  must be made inert, usually by setting its user data to NULL. Request
  handlers on it must then cope with NULL user data instead of dereferencing
  freed memory.
- A protocol error is sent with `wl_resource_post_error`. It queues
  `wl_display.error` and marks the client, and libwayland destroys the
  client only after the current request handler returns. Resources and their
  user data stay valid until then, so cleanup after the call is safe. The
  handler should return promptly without applying the request's effects.

## Event loop

- An fd, timer or signal source added to a `wl_event_loop` must be removed
  with `wl_event_source_remove` before the data it was given is freed.
- An idle source fires once, and libwayland removes and frees it after the
  callback returns. The callback must clear any stored pointer to it, and
  `wl_event_source_remove` is valid only while it is still pending.
  Removing one that already fired is a use after free.
