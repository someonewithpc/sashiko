# Userspace Review Prompts

Review prompts for userspace C and C++ on Linux, used by the `userspace`
project profile (`--project userspace`).

C and C++ share one profile. Library notes are picked per patch by the
paths and identifiers its diff touches, so a patch only gets the notes for
the libraries it uses.

## Layout

| Path | Loaded by |
|---|---|
| `review-core.md` | Every stage: what kind of code this is and where its sources are |
| `libraries/*.md` | Every stage, when the patch matches the note's `paths` or `symbols` |
| `error-handling.md`, `ownership.md`, `concurrency.md`, `device-interfaces.md` | The analysis stage that owns each concern |
| `false-positive-guide.md`, `severity.md` | The verification stages |
| `inline-template.md` | The report stage |

## Project context

These notes know libraries, not projects. Put what a reviewer of one
project needs to know (what the series does, which paths hold related
sources, which behaviour is intentional) in `--custom-prompt`.

## Writing a library note

A library whose API has its own ownership or completion rules, such as a
driver framework, deserves its own note.

Add `libraries/<name>.md`. It starts with a `+++` header giving a `title`
and the `paths` and `symbols` that select it, in the format
`src/notes/maintainer_notes.rs` describes. Name what a patch using the
library touches: a prefix such as `g_*`, its type names, the header name
an `#include` line leaves (`gio` for `<gio/gio.h>`). A note without a
matching path or symbol is never loaded.

State only what is true of the library or interface, with real function
and type names, and only what helps decide whether a change is wrong.
