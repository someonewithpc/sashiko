# False Positive Prevention Guide

## Core principle

**If you cannot prove an issue exists with concrete evidence from this
codebase, do not report it.**

Evidence means code you have read with the tools. Not "C code usually", not
"this looks like it could", not "a caller might". Name the function, the
path and the input that reaches it.

Proving a path is structurally possible is enough. You do not have to prove
it runs on every execution. A callback that can fire after its object is
freed is a bug even if the window is small.

## Patterns that produce false positives

### 1. Kernel rules applied to userspace

There is no GFP context, RCU, Kconfig, checkpatch, sleeping-in-atomic rule,
kernel coding style or Signed-off-by requirement here. A concern that only
exists under kernel rules is a false positive.

### 2. Allocation failure

`g_malloc`, `g_new` and `g_strdup` abort on failure. A missing NULL check
after them is not a bug. C++ `new` throws. A missing check after plain
`malloc` is a finding only if the project checks elsewhere or the size comes
from outside.

### 3. Defensive programming requests

Asking for a check that cannot fail is noise. If every caller passes a valid
pointer, a missing NULL check is not a bug. Prove an invalid value can reach
the code before reporting it. `g_return_if_fail` guards are optional
programmer-error checks; their absence is not a bug.

### 4. Cleanup that happens automatically

Before reporting a leak, check for `g_autoptr`, `g_autofree`, `g_auto`,
RAII members, smart pointers, a `GDestroyNotify` passed with the data, a
container created with a free function, and dispose or a destructor that
releases it. Before reporting a double free, check whether one of those is
paired with `g_steal_pointer` or `release()`.

### 5. Ownership by convention

Read the callee or its documentation before deciding who frees. A function
documented `(transfer full)` takes the value, and so does one that sinks a
floating reference, such as most functions that take a `GVariant`. A getter
documented `(transfer none)` does not give a reference.

### 6. Single-threaded event loops

Code that only runs in the main loop does not race with itself. A data race
needs two threads; show the second one. Ordering and reentrancy bugs inside
one thread are still real.

### 7. Build failures

A missing include, an undeclared function or a type error fails the build,
and the author would have seen it. Report a build problem only when it
appears in a configuration the author plausibly did not build: a meson
option turned off, another architecture, an older dependency version that
the build files still allow.

### 8. Library behaviour you have not checked

Do not claim a library function fails, blocks or allocates in a way its
documentation does not say. If the behaviour matters and the library source
is not in the checkout, say what would have to be true and how to check it.

### 9. Fixed later in the series

If other patches in the series are listed, check the series end state before
reporting a missing piece.

### 10. Style

Naming, formatting, comment wording and code organization are not findings,
unless a comment states something false about the code.
