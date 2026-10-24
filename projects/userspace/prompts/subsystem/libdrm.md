# libdrm: DRM/KMS from userspace

## Atomic commits

- `drmModeAtomicCommit` can fail for many reasons the program does not
  control: the configuration exceeds bandwidth or plane limits, a property
  value is out of range, the device lost DRM master (VT switch, another
  session), or a previous non-blocking commit is still pending (`EBUSY`).
  Every call must check the result and leave the program's view of the
  state matching what the kernel actually applied.
- A commit that changes a mode, enables or disables a CRTC, or moves a
  connector needs `DRM_MODE_ATOMIC_ALLOW_MODESET`; without it the kernel
  rejects the commit.
- `DRM_MODE_ATOMIC_TEST_ONLY` validates without applying. A test commit that
  passes can still fail when committed for real if something changed in
  between; the real commit's result still matters.
- Program state updated before a commit, and not rolled back when the commit
  fails, makes the next frame build on a configuration the kernel never
  accepted.
- Property IDs come from looking up names on each object
  (`drmModeObjectGetProperties` and `drmModeGetProperty`). They differ
  between devices and objects and must not be hardcoded or reused from
  another object.

## Events and lifetimes

- A page flip or commit requested with `DRM_MODE_PAGE_FLIP_EVENT` delivers an
  event later through `drmHandleEvent` on the DRM fd. The `user_data` passed
  with the commit must stay valid until that event arrives. Freeing an
  output, buffer or swapchain with a flip still pending is a use after free
  when the event comes in.
- Buffers scanned out by the current or pending commit must not be released
  or reused until the next flip completes.
- Framebuffers from `drmModeAddFB2` need `drmModeRmFB`, property blobs from
  `drmModeCreatePropertyBlob` need `drmModeDestroyPropertyBlob`, and every
  `drmModeGet*` result needs its `drmModeFree*`.

## Hotplug

- A hotplug uevent says something changed, not what. Connector state must be
  re-read; connectors can appear, disappear or change modes, and with
  DisplayPort MST connector IDs can be created and destroyed.
- `drmModeGetConnector` forces a probe, which can take a long time and wake
  sleeping displays. `drmModeGetConnectorCurrent` returns the cached state.
- Between the uevent and the query, the connector may have changed again.
  Code that indexes its own arrays by an earlier connector list must handle
  entries that vanished.

## Modes and properties

- A mode list can be empty for a connected connector (broken EDID).
- Enum and range property values must be checked against the property's
  definition before use; drivers expose different sets.
- Panel features exposed as connector properties (power saving, backlight,
  VRR) can be missing or read-only on other drivers and kernel versions.
