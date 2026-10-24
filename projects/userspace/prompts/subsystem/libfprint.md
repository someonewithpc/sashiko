# libfprint drivers

Driver code uses the internal `fpi_` API. Its ownership and completion rules
are strict, and breaking one usually shows up only on an error or
cancellation path.

## Actions

- Every action the core starts (open, close, enroll, verify, identify,
  capture, delete, list, clear storage) must be finished exactly once with
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

## State machines

- `fpi_ssm_start` takes ownership of the `FpiSsm`. It is freed after the
  completion callback runs, so the callback and anything after it must not
  free or reuse it, and nothing may keep a pointer to it.
- A state machine finishes once, through `fpi_ssm_mark_completed`,
  `fpi_ssm_mark_failed` or `fpi_ssm_next_state` from the last state. Each
  state handler must lead to exactly one of `fpi_ssm_next_state`,
  `fpi_ssm_jump_to_state`, `fpi_ssm_mark_completed` or `fpi_ssm_mark_failed`,
  now or from a callback it scheduled. Calling none stalls the device, and
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
