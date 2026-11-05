+++
title = "DejaGnu Testsuite"
paths = ["gcc/testsuite/", "libstdc++-v3/testsuite/"]
symbols = ["dg-*"]
+++
# DejaGnu Testsuite: maintainer notes

What GCC maintainers check in a new or changed test. The test's input is not reviewed as code, since it is meant to be odd or invalid. The directives are, because a test that cannot fail does not protect the fix.

- A test added for a fix must fail on the compiler without the fix. Check that its directives match where the diagnostic lands: `dg-error`, `dg-warning` and `dg-bogus` apply to the line the compiler reports, which is often the expression or declaration and not the call that triggered it. A `dg-bogus` on the wrong line passes without the fix, and only the generic "test for excess errors" catches the regression.
- A diagnostic reported on another line needs the line in the directive: `{ target *-*-* } .-1` for the line above, or an absolute line number.
- `c-c++-common/` tests run as C and as C++. A diagnostic only one front end emits needs `{ target c }` or `{ target c++ }`, and the two front ends often report different columns and wording, so match on a word both use.
- A test whose result depends on type sizes needs a target selector: `{ target lp64 }`, `{ target { lp64 || llp64 } }`, `int32plus` and so on. Without one it fails on ILP32 or 16-bit targets.
- A C++ test that needs a language level says so in `dg-do`: `{ target c++11 }`, `{ target c++20 }`. Without it the test also runs in C++98 mode, where the code may mean something else, such as a user-defined literal suffix lexing as a separate identifier.
- A test for a PR names it in its first comment, `// PR c++/NNNNNN`, and a regression test for a crash covers each entry point the fix covers, not only the one in the report.
- The ChangeLog lists the new test with `New test.` under `gcc/testsuite/ChangeLog`.
