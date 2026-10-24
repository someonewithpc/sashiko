# GLib

## Automatic cleanup

`g_autoptr(Type)`, `g_autofree`, `g_auto(Type)` and `g_autolist(Type)` use the
compiler's cleanup attribute. The cleanup runs when the variable goes out of
scope, on every path out of the block, including early returns and `goto`
past the end of the block.

- `g_autoptr(Type) var` frees with the function registered by
  `G_DEFINE_AUTOPTR_CLEANUP_FUNC(Type, func)`. GLib, GIO and most GObject
  types register one. A project type needs its own, usually through
  `G_DECLARE_FINAL_TYPE` or `G_DECLARE_DERIVABLE_TYPE`, which add it. A type
  without one fails to compile, which is not a review finding.
- `g_autofree` calls `g_free`. Using it on memory from `malloc` is fine with
  the default allocator. Using it on memory that needs a different free
  (`g_strfreev`, `g_object_unref`, `free` from another library's allocator)
  is a bug: `g_autofree gchar **v = g_strsplit (...)` leaks the strings.
  Use `g_auto(GStrv)` for that.
- `g_auto(Type)` is for stack values with a clear function, such as
  `g_auto(GValue)`, `g_auto(GVariantBuilder)` and `g_auto(GStrv)`. Lockers
  are pointers: `g_autoptr(GMutexLocker) locker = g_mutex_locker_new (&m)`.
- An automatic variable must be initialized at its declaration
  (`g_autoptr(GError) error = NULL;`). If a jump or early return can leave
  scope before the first assignment, an uninitialized one frees stack
  garbage.
- Returning an automatic variable needs `g_steal_pointer (&var)`, or the
  caller receives a pointer that was just freed. `return var;` on a
  `g_autoptr` or `g_autofree` variable is a use after free.
- Assigning a new value to an automatic variable that already holds one leaks
  the old value unless it is freed or cleared first (`g_clear_pointer`,
  `g_set_object`, `g_set_str`).
- Handing the value to a function that takes ownership (`transfer full`)
  needs `g_steal_pointer`. Otherwise both the callee and the cleanup free it.

## Clearing and stealing

- `g_clear_pointer (&p, free_func)` frees and sets `p` to NULL, and does
  nothing if `p` is already NULL. `g_clear_object`, `g_clear_error`,
  `g_clear_handle_id (&id, g_source_remove)` and
  `g_clear_signal_handler (&id, instance)` do the same for their types. Code
  that frees without clearing, in a struct field that dispose or a later call
  can reach again, is a double free waiting to happen.
- `g_steal_pointer (&p)` returns `p` and sets it to NULL.

## GError

- A function that takes `GError **error` sets it only on failure and then
  returns its failure value (FALSE, NULL or -1, whatever its documentation
  says). Callers check the return value, not whether `error` is set.
- An error may be set only once. Calling a second function with the same
  `&error` after the first one failed, without clearing it, triggers a
  critical warning and loses the second error. Inside a function, pass
  `error` straight through or use a local `g_autoptr(GError)` and
  `g_propagate_error`.
- Passing NULL for `error` is allowed and means the caller ignores the
  details. Dereferencing `*error` without checking that `error` is non-NULL is
  a crash for such callers.
- `g_prefix_error` and `g_propagate_prefixed_error` add context.
- An error code means something only within its domain. Testing
  `error->code == G_IO_ERROR_NOT_FOUND` without the domain also matches
  unrelated errors whose code has the same value; use
  `g_error_matches (error, G_IO_ERROR, G_IO_ERROR_NOT_FOUND)`.
- A function that both sets an error and returns success, or fails without
  setting one, breaks every caller that follows the convention.

## Preconditions and assertions

- `g_return_if_fail` and `g_return_val_if_fail` check programmer errors.
  They log a critical and return, and `G_DISABLE_CHECKS` compiles them out.
  They are not input validation: data from a device, file, D-Bus caller or
  user needs a real check.
- `g_assert` and `g_assert_not_reached` are compiled out under
  `G_DISABLE_ASSERT`. Code inside `g_assert` (a call with side effects) then
  disappears, and code after `g_assert_not_reached` must not rely on it
  aborting. The `g_assert_true`, `g_assert_cmpint` and other `g_assert_*`
  test macros stay.
- A `g_warning` or `g_critical` with no recovery after it lets execution
  continue into the bad state.

## Allocation and sizes

- `g_new (Type, n)`, `g_new0` and `g_renew` check `n * sizeof (Type)` for
  overflow and abort on it. `g_malloc (n * size)` does not check. A size
  computed from untrusted input needs an explicit overflow check or
  `g_malloc_n`.
- `g_malloc` aborts on failure; checking its result for NULL is dead code,
  not a bug. `g_try_malloc` can return NULL and must be checked.
- `gsize` is unsigned. Subtracting lengths without checking which is larger
  wraps to a huge value.
- `gint`, `guint` and `gsize` differ in width on 64-bit. Storing a `gsize`
  length in a `gint` truncates large inputs.

## Containers

- `g_hash_table_new_full` takes destroy functions for keys and values.
  `g_hash_table_insert` on an existing key keeps the old key, frees the new
  key with the key destroy function and replaces the value (freeing the old
  value). `g_hash_table_replace` keeps the new key and frees the old one. Code
  that still uses the key it passed in after `insert` can use freed memory.
- `g_hash_table_lookup` returns NULL both for a missing key and for a stored
  NULL value; `g_hash_table_lookup_extended` tells them apart.
- Modifying a hash table while iterating it with `GHashTableIter` is only
  allowed through `g_hash_table_iter_remove` and `g_hash_table_iter_replace`.
- A `GPtrArray` created with a free function frees elements on
  `g_ptr_array_remove*` and `g_ptr_array_unref`. `g_ptr_array_steal_index`
  returns an element without freeing it.
- `GArray` may move its data on append; pointers into `array->data` taken
  before an append can dangle.
- `GString` exposes `str` and `len`; `g_string_free (s, FALSE)` returns the
  buffer for the caller to free, `TRUE` frees it.

## Strings

- `g_strsplit` and similar return a `GStrv`, freed with `g_strfreev`.
- `g_strdup_printf` and `g_strconcat` return newly allocated strings.
  Passing one straight into a function that takes `const gchar *` leaks it.
- A format string from outside the program (`g_print (user_string)`) is a
  format string bug; use `"%s"`.
- `g_strlcpy` and `g_snprintf` truncate silently. A truncated path or
  command can name the wrong file.
- `g_utf8_validate` before `g_utf8_*` functions on data from outside.

## GVariant and GBytes

- `g_variant_new*` returns a floating reference. Passing it to a function that
  sinks it (most GVariant and GDBus API) hands it over. Keeping it needs
  `g_variant_ref_sink`. Unref'ing a floating variant that was already passed
  to a sinking function is a double unref.
- `g_variant_get` with `&s` borrows; with `s` it copies and the caller frees.
  A format string that does not match the variant's type is a programming
  error that aborts or logs a critical; check `g_variant_is_of_type` for
  data from outside.
- `g_bytes_get_data` borrows; the data lives as long as the `GBytes`.
