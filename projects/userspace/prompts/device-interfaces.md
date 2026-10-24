# Kernel and device interfaces from userspace

Hardware and the kernel are external input. A device can be unplugged at any
moment, firmware can return short or malformed data, and the kernel can
reject a request for reasons the program did not anticipate.

## ioctl

- Every `ioctl` can fail. Check the return value and `errno`; `EINTR`,
  `EAGAIN` and `ENODEV` (device gone) need handling, not a crash.
- Structures passed to an ioctl must be fully initialized, usually zeroed.
  Unset fields can carry garbage flags or pointers into the kernel.
- Field sizes and layouts come from the kernel UAPI headers. A structure
  declared by hand that does not match differs between 32-bit and 64-bit
  builds.

## sysfs and procfs

- Attribute values are text with a trailing newline. Exact comparisons
  (`strcmp`, `g_str_equal`) and strict parsers that require `*end == '\0'`
  (`g_ascii_string_to_signed`) fail unless it is stripped. `atoi` and
  `strtol` stop at the newline and parse fine, but `atoi` reports no errors,
  so a missing or garbage value reads as 0.
- An attribute can be missing on other hardware or kernel versions, can
  return `EIO` or `EINVAL` on read, and can reject a write. Writing a value
  the hardware does not support returns an error, which must not be ignored
  as success.
- A path built from a device name (`/sys/class/backlight/%s/brightness`) needs
  the name to come from enumeration, not from untrusted input that could
  contain `..` or `/`.

## udev and hotplug

- An add event can arrive before the device is fully set up (attributes not
  readable yet, permissions not applied); a remove event can arrive while an
  operation on the device is in flight. Code must handle both orders.
- After a remove, every pending operation on the device fails. Each failure
  path must not touch the freed device object.

## USB

- A transfer can complete with less data than requested. `actual_length`
  (libusb) or the equivalent in GUsb and libfprint's transfer helpers is
  the amount to parse, not the buffer size. Parsing a fixed-size structure
  out of a short transfer reads stale or uninitialized bytes.
- Lengths and offsets inside device data are untrusted. Check them against
  the received length before indexing.
- A transfer can time out, stall or fail with "no device". Cancelling a
  transfer still delivers its callback.

## DRM, input and other character devices

- `read` on an evdev or DRM fd returns whole events, possibly several. Code
  that assumes one event per read drops the rest.
- Device nodes opened by a privileged daemon on behalf of a user need an
  access check, or the daemon becomes a way around the node's permissions.
