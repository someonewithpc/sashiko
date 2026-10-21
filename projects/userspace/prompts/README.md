# Userspace Review Prompts

Review prompts for userspace C and C++ on Linux, used by the `userspace`
project profile (`--project userspace`). They cover GLib, GObject, GIO and
D-Bus, C++ lifetimes, Hyprland's hyprutils, DRM/KMS and device interfaces.

C and C++ share one profile. The pre-screen stage picks guides per patch
from `subsystem/subsystem.md`, so a C++ patch gets `cpp.md` and a GLib patch
gets `glib.md`, and neither reads the other.

## Layout

| Path | Loaded by |
|---|---|
| `review-core.md` | Every stage: what kind of code this is and where its sources are |
| `subsystem/subsystem.md` | The index the pre-screen stage selects guides from |
| `subsystem/*.md` | Library contracts and known bug patterns |
| `patterns/*.md` | The analysis stage that owns each concern |
| `false-positive-guide.md`, `severity.md` | The verification stages |
| `inline-template.md` | The report stage |

A guide is selected by name and looked up in both `subsystem/` and
`patterns/`, so names must be unique across the two directories.

## Project context

These guides know libraries, not projects. Put what a reviewer of one
project needs to know (what the series does, which paths hold related
sources, which behaviour is intentional) in `--custom-prompt`.

## Writing a new guide

Add the file, then add a row to `subsystem/subsystem.md`; an unlisted guide
is never selected. State only what is true of the library or interface, with
real function and type names, and only what helps decide whether a change is
wrong.
