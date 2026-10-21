# Guide Index

Load guides from the prompt directory based on what the change touches. Each
guide holds contracts and known bug patterns for one library or area.

A change can match several rows. Load **every** matching guide, not only the
most specific one. A GObject class that also exports a D-Bus interface matches
the GLib, GObject and GIO rows, and all three apply.

The triggers column lists headers, prefixes, type names and function names.
Matching any trigger is enough.

## Library Guides

| Library | Triggers | File |
|---------|----------|------|
| GLib | `#include <glib.h>`, `g_` functions, `gchar`, `gint`, `gsize`, `gboolean`, `GError`, `g_autoptr`, `g_autofree`, `g_auto`, `g_steal_pointer`, `g_clear_`, `GHashTable`, `GPtrArray`, `GArray`, `GString`, `GBytes`, `GVariant`, `g_return_if_fail`, `g_assert` | glib.md |
| GObject | `#include <glib-object.h>`, `GObject`, `G_DECLARE_`, `G_DEFINE_TYPE`, `_class_init`, `_init`, `dispose`, `finalize`, `g_object_ref`, `g_object_unref`, `g_object_new`, properties, `GParamSpec`, `g_signal_`, `g_signal_connect`, floating references, `GInitiallyUnowned` | gobject.md |
| GIO and D-Bus | `#include <gio/gio.h>`, `GTask`, `GCancellable`, `GAsyncResult`, `_async`, `_finish`, `GMainContext`, `GMainLoop`, `g_idle_add`, `g_timeout_add`, `GSource`, `GDBus`, `g_dbus_`, `GDBusMethodInvocation`, `g_bus_own_name`, polkit, `GFile`, `GInputStream`, `GSettings`, `GUdev`, `GUsb` | gio.md |
| C++ | `.cpp`, `.hpp`, `.cc`, `class`, templates, `std::`, `unique_ptr`, `shared_ptr`, `weak_ptr`, lambdas, iterators, move semantics, exceptions, Hyprland, aquamarine, hyprutils, `SP<`, `WP<`, `makeShared`, `CSignal`, `listen(` | cpp.md |
| DRM/KMS | `xf86drm`, `xf86drmMode`, `drmMode`, `drmIoctl`, atomic commits, `DRM_MODE_ATOMIC_`, page flips, CRTC, connector, plane, `drmModeGetConnector`, hotplug, `gbm_`, `EGL` | drm-kms.md |

## Cross-Cutting Patterns

| Pattern | Triggers | File |
|---------|----------|------|
| Error Handling | `goto out`, `goto err`, cleanup labels, return codes, `errno`, `-errno`, `NULL` returns, `GError`, partial reads or writes, `EINTR`, `EAGAIN` | error-handling.md |
| Memory and Ownership | `malloc`, `calloc`, `realloc`, `free`, `strdup`, `g_new`, `g_malloc`, `g_free`, transfer annotations, `(transfer full)`, `(transfer none)`, file descriptors, `close`, `mmap`, reference counts | ownership.md |
| Concurrency and Event Loops | `pthread`, `GThread`, `GMutex`, `std::thread`, `std::mutex`, atomics, signal handlers, callbacks, event loops, reentrancy, `g_main_context_invoke` | concurrency.md |
| Kernel and Device Interfaces | `ioctl`, `/sys/`, `sysfs`, `udev`, `evdev`, `libusb`, USB transfers, `actual_length`, `hidraw`, `/dev/`, `open(`, `read(`, `write(`, `poll(` | device-interfaces.md |
