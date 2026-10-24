# Userspace Review Prompts

Review prompts for userspace C and C++ on Linux, used by the `userspace`
project profile (`--project userspace`).

C and C++ share one profile. The pre-screen stage picks library guides per
patch from `subsystem/subsystem.md`, so a patch only gets the guides for the
libraries it touches.

## Layout

| Path | Loaded by |
|---|---|
| `review-core.md` | Every stage: what kind of code this is and where its sources are |
| `subsystem/subsystem.md` | The index the pre-screen stage selects guides from |
| `subsystem/*.md` | Library contracts and known bug patterns |
| `patterns/*.md` | The analysis stage that owns each concern |
| `false-positive-guide.md`, `severity.md` | The verification stages |
| `inline-template.md` | The report stage |

The pre-screen only selects from `subsystem/`. A pattern guide reaches
just the stage that owns it, so it is not listed in the index.

## Project context

These guides know libraries, not projects. Put what a reviewer of one
project needs to know (what the series does, which paths hold related
sources, which behaviour is intentional) in `--custom-prompt`.

## Writing a new guide

A library whose API has its own ownership or completion rules, such as a
driver framework, deserves its own guide.

Add the file, then add a row to `subsystem/subsystem.md`; an unlisted guide
is never selected. State only what is true of the library or interface, with
real function and type names, and only what helps decide whether a change is
wrong.
