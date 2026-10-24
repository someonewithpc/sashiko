# GObject

## Reference counting

- `g_object_new` returns a new reference owned by the caller, except for
  `GInitiallyUnowned` subclasses, which start floating. The first
  `g_object_ref_sink` takes ownership of a floating reference.
- Getters documented `(transfer none)` return a borrowed pointer that is valid
  only while its owner holds it. Storing it in a struct without
  `g_object_ref` dangles once the owner drops it.
- `(transfer full)` returns or parameters move a reference. A caller that
  also unrefs, or a callee that also refs, gets the count wrong.
- `g_set_object (&field, value)` refs the new value, unrefs the old one and
  handles `field == value`. Open-coded swaps often unref first and then ref
  an object that was just freed.
- A reference cycle between two objects (each holding a strong ref to the
  other) leaks both. One side needs a weak reference (`GWeakRef`,
  `g_object_add_weak_pointer`) or the cycle must be broken in dispose.

## Dispose and finalize

- `dispose` drops references to other objects and may run more than once
  (`g_object_run_dispose`, cycles). It must leave fields NULL after dropping
  them (`g_clear_object`, `g_clear_pointer`, `g_clear_handle_id`) so a second
  run does nothing.
- `finalize` frees memory that is not a reference to another object and runs
  once. Using another object in finalize that dispose already released is a
  NULL dereference if dispose cleared the field, or a use after free if it
  did not.
- Both must chain up: `G_OBJECT_CLASS (parent_class)->dispose (object)` at the
  end of dispose, and the same for finalize. A missing chain-up leaks the
  parent's state or skips its teardown.
- Sources (`g_timeout_add`, `g_idle_add`), signal connections on other
  objects, pending `GCancellable` operations and D-Bus registrations created
  by the object must be removed, disconnected or cancelled in dispose. A
  callback that fires after finalize uses freed memory.

## Signals

- `g_signal_connect (instance, "sig", cb, data)` keeps no reference to
  `data`. If `data` is freed while `instance` lives, the next emission is a
  use after free. Disconnect in the owner's dispose
  (`g_signal_handlers_disconnect_by_data`, `g_clear_signal_handler`) or use
  `g_signal_connect_object`, which disconnects when the object in `data`
  goes away.
- `g_signal_connect_swapped` swaps the first and last arguments of the
  callback. A callback written for the unswapped order reads the wrong
  pointers.
- Emission keeps the instance alive until `g_signal_emit` returns. Code after
  the emit that touches `self` is a use after free if a handler can drop the
  last external reference; the emitter then needs to hold its own reference
  around the emit.
- Handler signatures must match the signal's marshaller. A wrong return type
  on a signal with a return value (`gboolean` for event signals) is undefined
  behaviour.

## Properties

- `g_object_set_property`, `g_object_get` and `g_object_set` on a property
  name that does not exist log a `g_critical`. The name is a string checked
  at run time. The varargs `g_object_get` and `g_object_set` stop at the
  first bad name, so later out-parameters stay unwritten and later values
  unset.
- `g_object_get` returns copies or new references for strings, boxed types
  and objects; the caller frees them.
- With `G_PARAM_EXPLICIT_NOTIFY`, the setter must call
  `g_object_notify_by_pspec` only when the value changed. Without it, every
  set emits `notify`, even when the value did not change. That wakes every
  listener for nothing, and a handler that sets the property back can
  loop. `GBinding` blocks its own handler while it writes, so a plain
  binding does not loop.
- Construct properties are set after `_init` and before `constructed` runs.
  Code in `_init` that reads their fields sees zero, or whatever `_init`
  stored, not the `GParamSpec` default. Use `constructed` for setup that
  depends on them.

## Types

- `G_DECLARE_FINAL_TYPE` and `G_DEFINE_TYPE` must agree on names. Casting
  macros (`MY_OBJECT (obj)`) log a `g_critical` on a mismatch but still
  return the pointer, so a wrong cast reads the wrong memory in every build.
  `G_DISABLE_CAST_CHECKS` removes even the critical.
- Private data from `G_ADD_PRIVATE` is reached with
  `my_type_get_instance_private`; it is freed with the instance.
