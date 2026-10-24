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
| C++ | `.cpp`, `.hpp`, `.cc`, `class`, templates, `std::`, `unique_ptr`, `shared_ptr`, `weak_ptr`, lambdas, iterators, move semantics, exceptions | cpp.md |
| Hyprland and hyprutils | Hyprland, aquamarine, hyprutils, `Hyprutils::`, `SP<`, `WP<`, `makeShared`, `CSignal`, `listen(`, `HyprlandAPI::` | hyprutils.md |
| libwayland and wlroots | `wayland-server`, `wl_listener`, `wl_signal`, `wl_list`, `wl_resource`, `wl_client`, `wl_event_loop`, `wl_event_source`, `wlr_` | wayland.md |
| libdrm (DRM/KMS from userspace) | `xf86drm`, `xf86drmMode`, `drmMode`, `drmIoctl`, atomic commits, `DRM_MODE_ATOMIC_`, page flips, CRTC, connector, plane, `drmModeGetConnector`, hotplug, `gbm_`, `EGL` | libdrm.md |
