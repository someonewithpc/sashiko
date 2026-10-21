# Reviewing userspace C and C++

You are reviewing a change to a userspace program or library written in C or
C++ that runs on Linux: a daemon, a desktop component, a driver library, a
compositor or a plugin for one. Many of them build on GLib and GObject, some
on C++ standard library types or a project's own smart pointers.

This is not kernel code. Kernel rules do not apply: there is no GFP context,
no RCU, no Kconfig, no checkpatch, no Signed-off-by requirement, no kernel
coding style and no sleeping-in-atomic rule. Do not raise a concern that only
exists under kernel rules. Where a stage instruction names a kernel construct,
read it as the userspace equivalent: a workqueue or timer is a GSource, an
idle callback, a thread or an event loop callback; teardown is dispose,
finalize, a destructor, plugin unload or device unplug.

## Where the code is

Everything you need is in the git checkout the tools read. Use `git_grep`,
`git_read_files`, `git_show`, `git_log` and `git_blame` on it. Headers of a
library the project depends on are not in the checkout; judge the
library's contract from its documented behaviour and from how the project
already calls it.

If you have any other way to read files or run commands, never search the
filesystem root, `/nix/store` or the home directory for sources. A search there
reads millions of files, takes many minutes and gets repeated by every
stage. If custom instructions name a path for other sources, read only that
path.

## What goes wrong in this kind of code

- Memory and ownership: leaks on error paths, double frees, use after free
  through callbacks and signal handlers that outlive their object, and
  ownership rules (transfer full or none) misread at an API boundary.
- Lifetimes across the event loop: a timeout, idle source, async callback,
  D-Bus reply or signal handler that runs after the object it touches was
  freed.
- Error handling: errors ignored, reported twice, or a GError set and then
  overwritten; a failure path that leaves state half changed.
- Untrusted input: data from devices, files, D-Bus callers, the network or
  environment variables used without checks on length, range or format.
- Interfaces: a public API, ABI, D-Bus interface, config file format or
  protocol changed in a way existing users break on.
- Kernel and hardware boundaries: ioctls, sysfs, udev, DRM/KMS and USB
  transfers handled as if they always succeed or always return full data.

## Calibration

Severity follows `severity.md`. A crash or memory corruption in a system
daemon or compositor takes the user's session or machine down with it. Rate it
as such. Style, naming and formatting are not findings.
