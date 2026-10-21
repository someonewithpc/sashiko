# Severity Levels

Assign a severity to every finding. Use Medium as the default and move it up
or down with the questions below. Critical must be critical and High must be
damaging.

## Calibrating the level

State this reasoning at the start of `severity_explanation`.

- Consequence: what happens when the bug triggers (memory corruption, crash,
  data loss, security hole, leak, wrong result, slowdown). This sets the
  starting level.
- Triggering path: the concrete path that reaches the bug and the conditions
  it needs. If it rests on an assumption you could not verify, still report
  it and say so.
- Reachability: raise the level when untrusted input reaches the bug (device
  data, files, D-Bus callers, the network). Do not lower a level because you
  believe the path is rare; a wrong guess buries a real bug.

A speculative finding is capped at Medium, because the open question is
whether it is real. It is still reported.

## Critical

- **Question**: would it be better for the program to crash than to keep
  running?
- Memory corruption: buffer overflow, use after free, double free.
- Security holes: privilege escalation through a system daemon, missing
  authorization on a privileged D-Bus method, command injection.
- Data loss: a config or user file truncated or overwritten, enrolled
  fingerprints or other stored data destroyed.
- Public ABI or D-Bus API broken without a version change.

## High

- **Question**: can the program, the user's session or the device become
  unusable with a non-trivial probability?
- A crash or abort in a daemon, compositor or library on a reachable path.
- A deadlock or a main loop that stops dispatching.
- Wrong behaviour on a common path: the wrong power profile applied, a
  display left dark, a fingerprint accepted that should not match or never
  matching.
- Leaks on a path that repeats (every frame, every event, every hotplug).

## Medium

- Leaks on rare or one-time paths.
- Error handling that hides a failure but leaves the program working.
- A commit message that says something the code does not do.
- Performance regressions that users would notice but that do not break
  anything.
- Problems in tests and tooling.

## Low

- **Question**: is there any effect a user would see? If not, Low.
- Typos in comments or messages, misleading names, dead code.
- Negligible performance differences.
- Build problems in configurations nobody uses.
