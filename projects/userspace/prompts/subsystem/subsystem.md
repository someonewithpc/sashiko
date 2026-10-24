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
