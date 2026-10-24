# libfprint drivers

Driver code uses the internal `fpi_` API. Its ownership and completion rules
are strict, and breaking one usually shows up only on an error or
cancellation path.

## Actions

These rules apply to drivers that subclass `FpDevice` directly. Image
drivers follow the next section instead.

- Every action the core starts (probe, open, close, enroll, verify,
  identify, capture, delete, list, clear storage, suspend, resume) must be
  finished exactly once with
  the matching `fpi_device_*_complete` or `fpi_device_action_error`. Never
  finishing it leaves the caller waiting forever. A second call hits a
  `g_return_if_fail` on the current action and leaks the error it was given.
- The `GError` passed to a complete function is `(transfer full)`. Using or
  freeing it afterwards is a use after free or a double free.
- Verify and identify report their result with `fpi_device_verify_report` or
  `fpi_device_identify_report` before completing. A retry error (finger too
  short, not centred) is reported, then the action completes with no error.
  Only real failures go to the complete function as an error.
- `fpi_device_get_cancellable` gives the action's cancellable. Work that
  ignores it keeps running after the caller cancelled.

## Image drivers

- A driver that subclasses `FpImageDevice` implements `img_open`,
  `img_close`, `activate` and `deactivate` (and optionally `change_state`).
  The image core runs the `FpDevice` action and completes and reports it
  itself. Such a driver must not call `fpi_device_*_complete`,
  `fpi_device_action_error` or the verify and identify report functions;
  doing so finishes the action twice.
- Each vfunc is answered exactly once with its own function:
  `fpi_image_device_open_complete`, `fpi_image_device_close_complete`,
  `fpi_image_device_activate_complete` and
  `fpi_image_device_deactivate_complete`.
- During a capture the driver reports `fpi_image_device_image_captured` for
  a good image, `fpi_image_device_retry_scan` for a retryable one, and
  `fpi_image_device_session_error` for a fatal error. `session_error` takes
  the error `(transfer full)`, makes the core deactivate the device, and must
  not be used during deactivation.

## State machines

- `fpi_ssm_start` takes ownership of the `FpiSsm`. It is freed after the
  completion callback runs, so the callback and anything after it must not
  free or reuse it, and nothing may keep a pointer to it.
- A state machine finishes once, through `fpi_ssm_mark_completed`,
  `fpi_ssm_mark_failed` or `fpi_ssm_next_state` from the last state. Each
  state handler must lead to exactly one of `fpi_ssm_next_state`,
  `fpi_ssm_jump_to_state`, `fpi_ssm_mark_completed` or `fpi_ssm_mark_failed`,
  their `_delayed` variants, or `fpi_ssm_start_subsm`, now or from a
  callback it scheduled. Calling none stalls the device, and
  calling two runs a state twice or finishes twice.
- `fpi_ssm_mark_failed` takes the error `(transfer full)`. The completion
  callback also receives its error `(transfer full)` and must free or pass it
  on.

## USB transfers

- `fpi_usb_transfer_submit` takes ownership of the transfer and unrefs it
  after the callback returns. Keeping it beyond the callback needs
  `fpi_usb_transfer_ref`. `fpi_usb_transfer_submit_sync` does not take
  ownership; the caller unrefs.
- A transfer may be resubmitted only after its callback ran. Two submissions
  of one transfer at the same time hit a `g_return_if_fail`.
- The callback's error is `(transfer full)`. `fpi_ssm_usb_transfer_cb` hands
  it to `fpi_ssm_mark_failed`, so a custom callback that wraps it must not
  free it as well.
- `actual_length` can be shorter than the requested length on success. Code
  that parses the buffer must use `actual_length`, not the size it asked for.
  With `short_is_error` set, a short non-empty transfer fails before the
  callback runs, so on success `actual_length` is either the requested
  length or 0.
