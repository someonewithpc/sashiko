# Error handling in C and C++

## Trace every path out of a function

For each modified function, list the ways it can return: normal return, each
early return, each `goto` to a cleanup label, and, in C++, each call that can
throw. For each one, check what the caller sees and what state is left
behind.

- A resource acquired before the failure must be released on that path. A
  cleanup label sequence (`goto err_free_buf`) must release in reverse order
  of acquisition and must not release what was not acquired yet. Jumping to
  the wrong label leaks or double frees.
- State changed before the failure (a field set, a flag raised, an object
  added to a list, a property notified) must be undone or the object stays
  half updated.
- An output parameter must not be left pointing at freed memory on failure.

## Return conventions

Find the convention the project already uses and check that the new code
follows it on every path:

- `gboolean` plus `GError **`: FALSE with the error set on failure.
- Negative errno (`-EINVAL`): the caller checks `< 0`. Returning a positive
  errno, or `-1` and leaving the code in `errno`, mixes conventions.
- NULL on failure: the caller checks for NULL before use.
- `errno` after a libc call: it is meaningful only when the call reported
  failure, and any later libc call (including `close`, `free` on some
  platforms, logging) can overwrite it. Save it first.

A caller that ignores the return value of a function that can fail is a bug
when the failure matters: a `write` whose result is dropped, a `g_file_*`
call with a NULL error and no check, a `drmModeAtomicCommit` that may fail.

## Short operations and retries

- `read`, `write`, `recv`, `send` and stream reads can transfer fewer bytes
  than asked. Code that treats a partial result as complete truncates data.
- `EINTR` interrupts blocking calls when a signal arrives. `EAGAIN` and
  `EWOULDBLOCK` mean "try later" on non-blocking descriptors and are not
  errors to report.
- A retry loop needs a bound or a way out, or it spins on a permanent error.

## Integer handling

- Lengths and counts from outside the program must be range checked before
  they size an allocation, index an array or drive a loop.
- Mixed signed and unsigned comparisons convert the signed side when the
  unsigned type is `unsigned int` or wider (`size_t`, `guint`, `uint32_t`).
  `if (len - hdr < 0)` with such operands is always false. When both
  operands are narrower than `int` (`uint8_t`, `uint16_t`), both are
  promoted to `int` first, so the same check works.
- Narrowing a 64-bit size to `int` truncates. Functions like `snprintf`
  return `int` and a negative value on error.
- `strtol`, `strtoul` and `g_ascii_strtoll` report overflow through `errno`
  and the end pointer. Code that checks neither accepts garbage.
