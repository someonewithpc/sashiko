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
