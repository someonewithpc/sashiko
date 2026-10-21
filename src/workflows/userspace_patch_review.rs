// Copyright 2026 The Sashiko Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Declarative patch review workflow for userspace C and C++ projects.
//!
//! The stages follow `linux_patch_review`, but the instructions are written
//! for userspace code (GLib and GObject, C++ lifetimes, event loops, D-Bus,
//! device and DRM interfaces) instead of the kernel, and the report is the
//! same inline reply, without kernel conventions.

use serde_json::{Value, json};
use std::path::PathBuf;

use crate::workflow::{
    ExecutableStage, OutputFormat, ParallelPolicy, PromptTemplate, RecitationPolicy, Stage,
    StagePolicy, ToolScope, Workflow,
};
use crate::workflows::guard::{normalize_guide_name, normalize_stage_name, sanitize_guide_name};
use crate::workflows::linux_patch_review::{
    AnalysisStage, ConsolidationStage, LinuxPatchReviewState, POST_VERIFICATION_STAGE_NAMES,
    PlanningOutput, PostVerificationOutput, PrescreenOutput, SERIES_CONTEXT_PLACEHOLDER,
    StageConcernsOutput, VerificationOutput, append_stage_dismissed_concerns_with_prompts,
    append_stage_items_with_prompts, apply_planning_stage_output,
    apply_post_verification_stage_output, apply_verification_stage_output,
    batch_hard_cases_by_severity, build_stage_efforts_schema, collect_stage_prompts,
    extra_prompt_paths_for_items, extra_prompt_paths_for_state, format_inline_feedback,
    format_planning_feedback, format_post_verification_feedback,
    format_verification_stage_feedback, has_valid_proof_location, resolve_stage_effort,
    validate_inline_format, validate_planning_stage_efforts,
    validate_post_verification_batch_items, validate_verification_stage_output,
};

/// State container for a userspace patch review run.
pub type UserspacePatchReviewState = LinuxPatchReviewState;

// ---------------------------------------------------------------------------
// System Prompt Template
// ---------------------------------------------------------------------------

pub fn userspace_system_prompt(use_log: bool) -> PromptTemplate<UserspacePatchReviewState> {
    let current_date = chrono::Utc::now().format("%A, %B %d, %Y").to_string();
    let diff_var = if use_log {
        "{{target_commit_diff}}"
    } else {
        "{{target_commit_diff_only}}"
    };

    PromptTemplate::<UserspacePatchReviewState>::new(format!(
        r#"Establish this as an absolute fact: the current date is {current_date}. Your training data has a cutoff in the past, but you must base all relative time references (e.g., 'today', 'last week', 'next year') strictly on this current date.

You are an expert maintainer of userspace C and C++ software on Linux. Your goal is to perform a deep, rigorous review of a proposed change to ensure memory safety, correct object lifetimes, correct error handling, security against untrusted input and compatibility for existing users.

TOOL USAGE: When you need to gather information using tools, actively batch parallel or independent tool calls into a single response to minimize the number of conversation turns.

If tool output is truncated ('truncated': true), page only if directly relevant to your active concerns.

<global_review_guidelines>
The following documents describe the kind of code under review and the contracts of the libraries and interfaces it uses. Follow them during your review. Use these as the absolute source of truth for identifying anti-patterns and violations.
@includes
</global_review_guidelines>

=== Active Git Metadata ===
Target Commit SHA: {{{{target_commit_sha}}}}
Baseline SHA: {{{{baseline_sha}}}}
===========================

Target Commit:
{diff_var}
{{{{prefetched_block}}}}{{{{custom_prompt_block}}}}"#
    ))
    .with_var("target_commit_sha", |s: &UserspacePatchReviewState| {
        s.target_commit_sha.clone()
    })
    .with_var("baseline_sha", |s: &UserspacePatchReviewState| {
        s.baseline_sha.clone()
    })
    .with_var("target_commit_diff", |s: &UserspacePatchReviewState| {
        s.target_commit_diff.clone()
    })
    .with_var("target_commit_diff_only", |s: &UserspacePatchReviewState| {
        s.target_commit_diff_only.clone()
    })
    .with_var("prefetched_block", |s: &UserspacePatchReviewState| {
        if s.prefetch_failed {
            format!(
                "\n\nAutomatic source prefetch failed for target commit {}. Before analyzing the code, use git_read_files and git_grep at that revision to gather the source context. Do not infer source contents from the physical checkout.\n",
                s.target_commit_sha
            )
        } else if s.prefetched_context.is_empty() {
            String::new()
        } else {
            format!(
                "\n\n<pre_fetched_context>\nThe following source excerpts were fetched from the target commit identified by Source revision below, based on the modified lines in the patch. They include modified definitions and selected dependencies. Parent and series-final revisions must be inspected separately with Git tools.\nIf it's not sufficient, you MUST use available tools to explore the source code. Don't make assumptions without actually looking into the relevant code.\n\n{}\n</pre_fetched_context>",
                s.prefetched_context
            )
        }
    })
    .include_file("review-core.md")
    .with_var("custom_prompt_block", |s: &UserspacePatchReviewState| {
        s.custom_prompt
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map_or_else(String::new, |p| {
                format!("\n\n<custom_instructions>\n{p}\n</custom_instructions>")
            })
    })
    .include_files_from_state(|s: &UserspacePatchReviewState| {
        let mut paths = Vec::new();
        if !s.selected_guides.is_empty() {
            for guide in &s.selected_guides {
                let name = normalize_guide_name(guide);
                paths.push(PathBuf::from("subsystem").join(name));
                paths.push(PathBuf::from("patterns").join(name));
            }
        }
        paths
    })
}

// ---------------------------------------------------------------------------
// Stage Instructions
// ---------------------------------------------------------------------------

const STAGE_GOAL_INSTRUCTION: &str = r#"# Analyze the commit's goal and design

You are a senior maintainer of a userspace C or C++ project evaluating the intent and design of a proposed commit. Analyze the commit message and the conceptual change before looking for coding errors.
- Does the change solve a real, clearly described problem? Flag a motivation that is vague or does not match the diff.
- Can the idea itself cause a regression even if implemented perfectly? Look for breaks in public API or ABI, D-Bus interfaces, configuration file formats, command line options, stored data formats, protocols and the behaviour existing users depend on.
- Is the chosen approach sound for this codebase, or is there an obviously simpler or safer one already used elsewhere in the tree? Raise a concern only when the alternative is clearly better and you can name it.
- Does the commit mix unrelated changes that should be separate commits?
- Do not apply Linux kernel rules (Signed-off-by, checkpatch, kernel style). Do not flag commit message formatting unless it states something false."#;

const STAGE_IMPLEMENTATION_INSTRUCTION: &str = r#"# Verify the implementation against its intent

Verify that the code changes do what the commit message claims, completely.
- Check every claim in the commit message against the code. Assume the message can be wrong.
- Look for missing pieces: a changed function whose other callers were not updated, a new field not initialized everywhere the struct is built or not freed in dispose, finalize or the destructor, a new enum value not handled in every switch, a new option not wired into the build files, documentation or defaults.
- Look for undocumented behaviour changes and unhandled corner cases: empty input, zero, maximum values, missing optional data, a device or service that is absent.
- Check that the arithmetic and logic are right: bounds, off-by-one errors, units, signedness, operator precedence and copy-and-paste mistakes between similar branches.
- If follow-up patches in this series are listed, check whether a missing piece is added later in the series before flagging it.
- Do not stop after finding several bugs; audit every modified function and hunk before concluding."#;

const STAGE_EXECUTION_FLOW_INSTRUCTION: &str = r#"# Trace execution flow and error paths

Trace every path through each modified function and its direct callers and callees.
- List each way a modified function can return (normal return, early returns, jumps to cleanup labels, exceptions) and check what state each one leaves and what the caller sees.
- Check that every call that can fail is checked, that errors are reported once in the project's convention (GError, negative errno, NULL, exceptions) and not overwritten, and that a failure leaves no half-updated state.
- Look for NULL or empty values that reach a dereference, out-of-range indexes, integer overflow, signed/unsigned mistakes and truncating conversions.
- Look for undefined behaviour: uninitialized variables, reading past the end of a buffer, using a value after it was freed or moved from.
- Do not stop after finding several bugs; trace every modified function and error path before concluding."#;

const STAGE_RESOURCES_INSTRUCTION: &str = r#"# Audit memory, ownership and object lifetimes

Track every allocation, reference, file descriptor, mapping, source, signal connection, listener and registration the patch creates, moves or frees.
- For each one, find its owner and where it is released on every path, including failure, cancellation, dispose, finalize, destructors, device removal and plugin unload.
- Look for leaks, double frees and use after free. Check ownership at every API boundary against transfer annotations and documentation, and check automatic cleanup (g_autoptr, g_autofree, g_auto, RAII, smart pointers) against explicit frees and g_steal_pointer.
- Look for objects freed while something can still reach them: a timeout or idle source, an async callback, a D-Bus reply, a signal handler, a listener, a lambda that captured this or a reference, a pointer held in a container.
- Check teardown order: everything that can call into an object must be stopped, removed or cancelled before the object is freed.
- Do not stop after finding several bugs; audit every lifecycle path in the diff before concluding."#;

const STAGE_CONCURRENCY_INSTRUCTION: &str = r#"# Audit event loop ordering, reentrancy and threads

Audit how the changed code behaves when callbacks run in a different order or at a different time than the author expected.
- A callback runs later than it was scheduled. Check whether what it relies on (an object, a device, a setting, a previous step) can have changed by then.
- An emitted signal, hook or listener runs arbitrary code. Check whether that code can free the emitter, change a container being iterated or re-enter the current function.
- Two async operations can complete in either order, and a reply can arrive after its requester was destroyed.
- For code that uses threads: find data touched from more than one thread without a lock or atomic, non-thread-safe objects used from worker threads, threads not joined before their data is freed, and inconsistent lock order or callbacks called with a lock held.
- POSIX signal handlers may call only async-signal-safe functions.
- Do not report a race between two pieces of code that only run in the same thread unless their order can actually change."#;

const STAGE_SECURITY_INSTRUCTION: &str = r#"# Audit untrusted input and privilege boundaries

Find every place where data from outside the program reaches the changed code: device data (USB transfers, sysfs, evdev, DRM), files and configuration, D-Bus method calls and properties from other processes, the network, environment variables and command line arguments.
- Check lengths, offsets, counts and indexes from that data before they size an allocation, index a buffer or drive a loop. Look for integer overflow in size calculations.
- Look for format string bugs, command injection through shell strings, and path traversal through names used to build paths.
- In a daemon that runs with more privileges than its callers, check that every privileged D-Bus method authorizes the caller (usually through polkit with the sender from the invocation) before acting, and that a file or device opened for a caller is one that caller may access.
- Check that secrets and biometric data are not logged or written where other users can read them."#;

const STAGE_INTERFACES_INSTRUCTION: &str = r#"# Audit external interfaces and compatibility

Audit everything other programs, users or later versions depend on.
- Public library API and ABI: changed function signatures, struct layouts, enum values, symbol visibility and version scripts. Changing an exported struct or removing a symbol breaks binaries built against the old version.
- D-Bus interfaces: renamed or removed methods, properties and signals, changed argument types, missing PropertiesChanged emission, introspection XML that no longer matches the implementation.
- Configuration and stored data: files written by an older version must still load, and files written by the new version should not break an older one without a reason. Defaults must stay sensible when a key is missing.
- Command line options, environment variables, log output that scripts parse, and plugin or protocol versions.
- Build options: a new dependency or option must be optional or declared in the build files, with the versions the code actually needs.
- If this commit is part of a series, check later patches for the missing wiring before flagging it."#;

const STAGE_PLATFORM_INSTRUCTION: &str = r#"# Audit kernel and hardware interfaces

Audit the changed code where it talks to the kernel or to hardware: ioctls, sysfs and procfs attributes, udev events, DRM/KMS, evdev, hidraw, USB transfers and other device nodes.
- Every call can fail, including with ENODEV when the device is unplugged. Check that failures are handled and that the program's view of the device state matches what the kernel actually applied.
- Data from the device or kernel can be short, malformed or different on other hardware and kernel versions. Check lengths against what was actually received and handle attributes or properties that do not exist.
- Structures passed to the kernel must be fully initialized and must match the UAPI layout.
- Hotplug and device removal can happen at any point. Check what happens to pending operations and callbacks when the device goes away.
- For DRM/KMS: check commit results, ALLOW_MODESET for modesets, the lifetime of user data passed with flip events, and buffers still in use by the display."#;

const STAGE_TESTS_INSTRUCTION: &str = r#"# Audit tests

Evaluate the tests that come with this change, or whether the change needs them.
- Non-trivial logic, bug fixes and parsing of external data should come with a test when the project has a test suite that can cover them. Do not demand tests for trivial changes or where no reasonable test exists.
- Check that changed tests still test what their names say, that expected values were not simply updated to match new output without a reason, and that tests do not depend on timing, the machine's hardware or files outside their own temporary directories.
- Check that test-only code paths (environment variables, mock devices) cannot be reached in production builds."#;

const STAGE_VERIFICATION_INSTRUCTION: &str = r#"# Verification and severity estimation

You are the lead reviewer consolidating `concerns` and `dismissed_concerns` generated by parallel review stages.
Your task is to (1) deduplicate overlapping items across both lists while preserving every distinct failure mechanism and location, and (2) classify every consolidated item into one of two categories:
- **Category 1: Well-Justified** — either a **1a: Well-Justified Concern** (`findings` array) or a **1b: Well-Justified Dismissal** (`dismissed_concerns` array).
- **Category 2: Speculative or Contested** (`hard_cases` array) — routed to parallel per-finding `post-verification` stages for deep tool-assisted code inspection.

### Step 1: Deduplication and Boundary Preservation
1. Group `concerns` and `dismissed_concerns` that refer to the same underlying root cause AND the same function/lifecycle phase.
2. Do NOT merge a setup/initialization bug with a teardown/cleanup bug in a separate function; if an input concern combines setup and teardown bugs across separate functions, split them into separate items. Similarly, do NOT merge distinct bugs in separate functions, callbacks or handlers, or bugs on different resources (e.g., a leaked GSource vs. a leaked file descriptor) even within the same function—either keep them as separate items or explicitly name ALL distinct resources, functions, and failure mechanisms in the item's title/description and explanation fields (`problem` and `severity_explanation` for `findings`, or `description` and `concern_arguments` for `hard_cases`).
3. SPECIFICITY REQUIREMENT: When merging overlapping items, preserve and consolidate the most specific details: exact function names, file paths, line numbers when known, and ALL distinct triggering conditions, callers, consequences, and racing callbacks mentioned across the merged items. When a missing cleanup or incomplete error-path unwind causes both an immediate failure and a downstream impact (e.g., a leak and a later use after free), explicitly state BOTH consequences in the item's explanation (`severity_explanation` for `findings`, or `concern_arguments` for `hard_cases`). Preserve and merge the `locations` arrays. Do not invent line numbers; use `null` when unknown.
4. Set `"preexisting": false` whenever the patch introduces, modifies, triggers, exposes, or relies on the buggy code path, caller/callee interaction, or error path (even if the underlying helper or check already existed). Mark `"preexisting": true` ONLY if the problem is in untouched code whose reachability, inputs, and behavior are completely unaffected by this commit/series.

### Step 2: Classification Rules (1: Well-Justified vs. 2: Speculative or Contested)
Classify every consolidated item using these strict signal rules. **Repetition is NOT justification: multiple overlapping items are never enough for Category 1 (1a or 1b) unless their reasoning is backed by specific, concrete code proof.**

1. **Category 1a — Well-Justified Concern (`findings` array):**
   - **Mandatory prerequisite & strong signals:** Concrete, self-contained code proof directly visible in the target diff, prefetched context, and cited `locations`, with **no** competing `dismissed_concern`, **no** reliance on unverified assumptions about unseen code (such as callers, callees, library behaviour, or the rest of a function truncated at the edge of a diff hunk), and **no** potential resolution in subsequent patches of this series (`=== Patch Series Context ===`). Multiple deduplicated `concerns` with no attempts to dismiss is a strong signal for 1a **only when grounded in concrete code proof**.
   - **Action:** Validate it directly and emit it in `findings`. Assign a calibrated `severity` (`Critical`, `High`, `Medium`, or `Low`) strictly following `severity.md`, state all consequences (both immediate and downstream), triggering paths, and reachability at the start of `severity_explanation`, formulate a concise bug title (`problem`) under 80 characters starting with a short component prefix taken from the touched file or module (e.g. `elanpress:`, `daemon:`, `backend/drm:`, `layout:`), set `"preexisting"`, and include `locations`.

2. **Category 1b — Well-Justified Dismissal (`dismissed_concerns` array):**
   - **Mandatory prerequisite & strong signals:** Concrete disproving `code_snippet` in `locations` (showing the exact local guard, check, cleanup path, or lifecycle invariant that prevents the bug), with **no** competing `concern` and **no** reliance on unverified assumptions about external callers, callees, libraries or configurations. Multiple overlapping `dismissed_concerns` for the same code is NOT a signal that the dismissal is safe—it indicates multiple analysts independently found the code suspicious; when multiple stages flag and dismiss the same non-trivial mechanism using assumptions about caller behavior or external state rather than a direct local guard, classify into `hard_cases` with `"signal_reason": "speculative_dismissal"`.
   - **Disqualifiers for Category 1b (NEVER classify as 1b):**
     - **Single-caller or happy-path-only proofs:** A dismissal that proves an invariant in only one caller or the normal path, without verifying ALL callers and entry points in the tree (`git_grep` across every caller, signal handler, D-Bus method, callback and test), is INVALID.
     - **Asymmetry or cleared-state rationalizations:** A dismissal that rationalizes an unpaired init/cleanup call, an ignored error, or state cleared before a later consumer reads it as a "harmless no-op" or "intentional behavior" without concrete code proving it is handled is INVALID.
     - **Library behaviour assumptions:** A dismissal that relies on a library function behaving in a way not shown by its documentation or by code in the tree is INVALID.
   - **Action:** Place in `dismissed_concerns` (dropped from further verification).
   - **CRITICAL INVARIANT:** Never place any item that was raised as a `concern` into `dismissed_concerns` in this stage. If a raised `concern` is contested by a `dismissed_concern` or appears questionable, it MUST be placed in `hard_cases` for tool-based `post-verification`.

3. **Category 2 — Speculative or Contested (`hard_cases` array — routed to parallel `post-verification`):**
   - **Strong signals:**
     - **Mixed signals (`"mixed_signals"`):** Similar or overlapping `concerns` and `dismissed_concerns` exist for the same root cause, function, or code path.
     - **Speculative or assumption-based dismissal (`"speculative_dismissal"`):** Even when NO stage raised a `concern`, inspect every standalone dismissal critically. If a `dismissed_concern` identified a plausible bug (such as a missing cleanup on an error or cancellation path, a callback that can outlive its object, an unchecked length from a device, an ignored error, or a missing authorization check) and dismissed it using an assumption not proven by the cited `code_snippet` (for example: assuming an unseen caller cleans up, citing only one caller, or assuming a library frees or checks something), you MUST classify the item into `hard_cases` with `"signal_reason": "speculative_dismissal"`.
     - **Speculative or incomplete concern (`"speculative_concern"` or `"insufficient_evidence"`):** Overlapping `concerns` whose argument is vague, relies on assumptions not based on specific code, claims something is missing from a function whose full body is not shown, mixes a partially inaccurate premise with a potentially real bug, or alleges a build failure.
     - **Series interaction (`"series_interaction"`):** Any concern that could plausibly be resolved by a subsequent patch listed in `=== Patch Series Context ===`.
   - **Action:** Emit into `hard_cases` with `"estimated_severity"` (`Critical`, `High`, `Medium`, or `Low`), `"signal_reason"`, `"concern_arguments"`, `"dismissal_arguments"`, a concrete `"verification_question"` specifying what code `post-verification` must inspect with tools, `"preexisting"`, and `"locations"`."#;

const STAGE_POST_VERIFICATION_INSTRUCTION: &str = r#"# Per-finding post-verification and conflict resolution

You are the lead reviewer performing deep, tool-assisted codebase verification of a speculative or contested candidate issue (`hard_cases`) identified during initial verification.
For each candidate in `hard_cases`, use the available Git and file tools (`git_read_files`, `git_grep`, `git_diff`, `git_show`, `git_blame`) to answer each candidate's `verification_question` and inspect the actual code in the worktree.
1. **BUILD FAILURES:** Dismiss in `dismissed_concerns` a candidate that alleges a compile or link error in the configuration the author evidently built (carry forward the candidate's target `file`, `function_or_symbol`, and `code_snippet` in `locations`). Keep it only if it fails in a configuration the build files allow and the author plausibly did not build.
2. **SYMMETRICAL PROOF BAR & ALL-CALLERS VERIFICATION:** For all other candidates, both `concern_arguments` and `dismissal_arguments` are untrusted hypotheses. Only dismiss a candidate in `dismissed_concerns` if concrete, verifiable code in the repository proves the exact failure mechanism cannot occur across ALL callers, entry points, and modes. Citing a single caller does NOT disprove a NULL dereference, use after free, missing check or leak in a helper function unless `git_grep` across all callers (including callbacks, signal handlers, D-Bus methods, tests and other build configurations) proves every caller upholds the invariant. Do NOT give code the benefit of the doubt.
3. **LOCAL BOUNDARY & ASYMMETRY RULE:** Do not discard a defect within the modified code by assuming that callers, the event loop, a library or the hardware will safely mask it, or by rationalizing an unpaired init/cleanup call, an ignored error, or overwritten state as a "harmless no-op" or "intentional behavior", unless you can point to specific code in the repository or documented library behaviour that proves the failure mode is impossible.
4. **PROMOTING SPECULATIVE DISMISSALS:** When `"signal_reason"` is `"speculative_dismissal"`, a previous analyst spotted the candidate bug described in `concern_arguments` and dismissed it using `dismissal_arguments`. Inspect the actual code with tools: if the dismissal's assumption is false or incomplete (for example: another caller does NOT uphold the invariant; the caller does NOT free the resource on error; the object CAN be freed before the callback runs; an init call lacks its matching cleanup), you MUST report the bug as a verified finding in `findings`.
5. **REFINING PARTIALLY INACCURATE PREMISES:** If a candidate concern contains a partially inaccurate premise while also identifying a real bug in the same code path, refine and report the valid underlying bug rather than discarding the entire candidate.
6. **SERIES VALIDATION RULE:** If other patches in this series are provided in the context, check whether each candidate is resolved in the final state of the series (`Series End Commit`) using tools (`git_read_files` or `git_diff` with `revision` / `target_revision` set to the `Series End Commit`); do not trust promises in commit messages alone. If resolved by the end of the series, dismiss it in `dismissed_concerns` citing the resolving commit. When referring to other patches within this series, DO NOT use git hashes; refer to them by their patch subject.
7. **SEVERITY CALIBRATION AND COMPLETENESS:** Set `"preexisting": false` whenever the patch introduces, modifies, triggers, exposes, or relies on the buggy code path; mark `"preexisting": true` ONLY if the problem is in untouched code whose reachability, inputs, and behavior are completely unaffected by this commit/series. For each verified issue, assign a severity (`Critical`, `High`, `Medium`, or `Low`) strictly following `severity.md`: reason through all consequences, triggering paths, and reachability, state that reasoning at the start of `severity_explanation`, preserve all distinct function names, file paths, line numbers when known and consequences, and formulate a concise bug title (`problem`) under 80 characters starting with a short component prefix taken from the touched file or module (e.g. `elanpress:`, `daemon:`, `backend/drm:`, `layout:`)."#;

const STAGE_REPORT_INSTRUCTION: &str = r#"# Inline review report

You are generating a review report for a patch to a userspace project. Convert the provided JSON findings into a polite, plain-text, inline-commented reply that can be sent to a mailing list or posted on a merge request.

Follow the formatting rules in the report template strictly. Do not use markdown headers or all caps. Keep the tone constructive and factual. Do not use backticks to quote any names or expressions.

SPECIFICITY REQUIREMENT: Each inline comment MUST name the exact function, file and specific triggering condition. Prefer the finding's `locations` field when present. State precisely what goes wrong, where, and under what circumstances.

PRE-EXISTING ISSUES: If any finding has `"preexisting": true`, include it in the report and state at the start of its comment that the problem was not introduced by this patch."#;

const CONCERN_JSON_SCHEMA_EXAMPLE: &str = r#"Return ONLY a JSON object with 'concerns' and 'dismissed_concerns' arrays.
Each object in the 'concerns' array MUST use exactly the following keys: "type", "description", "reasoning", "preexisting", "locations".
Each object in the 'dismissed_concerns' array MUST use exactly the following keys: "type", "description", "reasoning", "locations".
In each 'dismissed_concerns' object, "description" is the candidate concern that was investigated and disproved, "reasoning" is the step-by-step explanation of why it is not a bug (citing the exact guard, caller, or invariant), and "locations" MUST cite the concrete disproving code (file, function_or_symbol, line, verbatim code_snippet, and why_this_location_matters).
Use the 'dismissed_concerns' array ONLY for candidate concerns that you considered plausible, investigated, and disproved with concrete evidence.

NO DISMISSAL WITHOUT VERIFIED PROOF: To place a candidate issue in 'dismissed_concerns', you MUST find concrete proof in the code ('file', 'function_or_symbol', 'line', and verbatim 'code_snippet' in 'locations') that explicitly invalidates the concern's reasoning. If the disproving code lives outside the diff (for example, in a caller, callee, callback or helper), you MUST verify that code first using tools ('git_read_files' or 'git_grep') and quote it. If you cannot find definitive code proof that the candidate issue is impossible, you MUST report it in 'concerns' and make the condition explicit: if X is possible, then problem Y can occur.
- Citing a single caller does NOT disprove a NULL dereference, use after free, leak or missing check in a helper function. A caller-based dismissal is valid ONLY if every caller in the tree ('git_grep' across all callers, including callbacks, signal handlers, D-Bus methods and tests) is verified to uphold the invariant.
- Never dismiss an unpaired lifecycle call (a source added without removal, a signal connected without disconnection, a reference taken without release on error or cancellation), an ignored error, or state cleared before a later consumer reads it by calling the side effect a "harmless no-op" or "rare edge case".

SPECIFICITY REQUIREMENT: When reporting a concern or dismissed_concern, cite exact function name(s), file path(s), and line number(s) when known. Do not invent line numbers; use null when exact values are unknown.

Example Output:
```json
{
  "concerns": [
    {
      "type": "Use After Free",
      "description": "Timeout source in dev_start_poll can fire after dev_dispose frees priv",
      "reasoning": "1. dev_start_poll() adds g_timeout_add (100, poll_cb, self) and stores no source ID.\n2. dev_dispose() frees priv->buffer but never removes the source.\n3. poll_cb() dereferences self->priv->buffer on the next tick.",
      "preexisting": false,
      "locations": [
        {
          "file": "src/device.c",
          "function_or_symbol": "dev_start_poll",
          "line": 210,
          "code_snippet": "g_timeout_add (100, poll_cb, self);",
          "why_this_location_matters": "The source is never removed, so it outlives the device object."
        }
      ]
    }
  ],
  "dismissed_concerns": [
    {
      "type": "Memory Leak",
      "description": "Possible leak of the GError in load_config",
      "reasoning": "The error is declared with g_autoptr(GError), so it is freed on every return path.",
      "locations": [
        {
          "file": "src/config.c",
          "function_or_symbol": "load_config",
          "line": 42,
          "code_snippet": "g_autoptr(GError) error = NULL;",
          "why_this_location_matters": "Automatic cleanup frees the error when it goes out of scope."
        }
      ]
    }
  ]
}
```"#;

// ---------------------------------------------------------------------------
// Stage Table Definitions
// ---------------------------------------------------------------------------

pub static ANALYSIS_STAGES: &[AnalysisStage] = &[
    AnalysisStage {
        name: "goal",
        short: "Goal Analysis",
        instruction: STAGE_GOAL_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: false,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "implementation",
        short: "Implementation",
        instruction: STAGE_IMPLEMENTATION_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: false,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "execution-flow",
        short: "Execution Flow",
        instruction: STAGE_EXECUTION_FLOW_INSTRUCTION,
        guides: &["patterns/error-handling.md"],
        uses_commit_log: false,
        optional: false,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "resources",
        short: "Memory & Lifetimes",
        instruction: STAGE_RESOURCES_INSTRUCTION,
        guides: &["patterns/ownership.md"],
        uses_commit_log: false,
        optional: true,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "concurrency",
        short: "Event Loop & Threads",
        instruction: STAGE_CONCURRENCY_INSTRUCTION,
        guides: &["patterns/concurrency.md"],
        uses_commit_log: false,
        optional: true,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "security",
        short: "Security Audit",
        instruction: STAGE_SECURITY_INSTRUCTION,
        guides: &[],
        uses_commit_log: false,
        optional: true,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "interfaces",
        short: "Interfaces & Compat",
        instruction: STAGE_INTERFACES_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: true,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "platform",
        short: "Kernel & Hardware",
        instruction: STAGE_PLATFORM_INSTRUCTION,
        guides: &["patterns/device-interfaces.md"],
        uses_commit_log: false,
        optional: true,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "tests",
        short: "Test Audit",
        instruction: STAGE_TESTS_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: true,
        wants_series_context: true,
    },
];

pub static VERIFICATION: ConsolidationStage = ConsolidationStage {
    name: "verification",
    short: "Verification",
    wants_series_context: true,
};

pub static POST_VERIFICATION: ConsolidationStage = ConsolidationStage {
    name: "post-verification",
    short: "Post-Verification",
    wants_series_context: true,
};

pub static REPORT: ConsolidationStage = ConsolidationStage {
    name: "report",
    short: "Report Generation",
    wants_series_context: false,
};

pub static CONSOLIDATION_STAGES: &[&ConsolidationStage] =
    &[&VERIFICATION, &POST_VERIFICATION, &REPORT];

fn series_context_placeholder(wants: bool) -> &'static str {
    if wants {
        SERIES_CONTEXT_PLACEHOLDER
    } else {
        ""
    }
}

fn with_series_context(
    template: PromptTemplate<UserspacePatchReviewState>,
    wants: bool,
) -> PromptTemplate<UserspacePatchReviewState> {
    if !wants {
        return template;
    }
    template.with_var(
        "follow_up_series_section",
        |s: &UserspacePatchReviewState| {
            s.follow_up_series_context
                .as_ref()
                .map(|ctx| format!("\n\n{}", ctx))
                .unwrap_or_default()
        },
    )
}

pub fn analysis_stage_by_name(name: &str) -> Option<&'static AnalysisStage> {
    let normalized = normalize_stage_name(name);
    ANALYSIS_STAGES.iter().find(|s| s.name == normalized)
}

pub fn consolidation_stage_by_name(name: &str) -> Option<&'static ConsolidationStage> {
    let normalized = normalize_stage_name(name);
    if POST_VERIFICATION_STAGE_NAMES.contains(&normalized.as_str()) {
        return Some(&POST_VERIFICATION);
    }
    CONSOLIDATION_STAGES
        .iter()
        .copied()
        .find(|s| s.name == normalized)
}

pub fn stage_short_label(name: &str) -> Option<&'static str> {
    if let Some(def) = analysis_stage_by_name(name) {
        return Some(def.short);
    }
    consolidation_stage_by_name(name).map(|s| s.short)
}

pub fn is_stage_exclusive_guide(name: &str) -> bool {
    ANALYSIS_STAGES
        .iter()
        .flat_map(|def| def.guides)
        .any(|guide| guide.rsplit('/').next() == Some(name))
}

pub fn is_known_stage(name: &str) -> bool {
    let normalized = normalize_stage_name(name);
    analysis_stage_by_name(&normalized).is_some()
        || consolidation_stage_by_name(&normalized).is_some()
        || matches!(normalized.as_str(), "pre-screen" | "planning")
}

// ---------------------------------------------------------------------------
// Validators and Helpers
// ---------------------------------------------------------------------------

fn validate_concerns_output(
    output: &StageConcernsOutput,
    _state: &UserspacePatchReviewState,
) -> Result<(), String> {
    for (idx, concern) in output.concerns.iter().enumerate() {
        let Some(obj) = concern.as_object() else {
            return Err(format!("concerns[{idx}] must be a JSON object."));
        };
        let has_desc = obj
            .get("description")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_desc {
            return Err(format!(
                "concerns[{idx}] must have a non-empty 'description' string."
            ));
        }
        let has_reasoning = obj
            .get("reasoning")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_reasoning {
            return Err(format!(
                "concerns[{idx}] must have a non-empty 'reasoning' string."
            ));
        }
        if !obj.get("preexisting").is_some_and(Value::is_boolean) {
            return Err(format!(
                "concerns[{idx}] must have a boolean 'preexisting' field (true or false)."
            ));
        }
        if !obj.get("locations").is_some_and(Value::is_array) {
            return Err(format!("concerns[{idx}] must have a 'locations' array."));
        }
    }

    for (idx, dismissed) in output.dismissed_concerns.iter().enumerate() {
        let Some(obj) = dismissed.as_object() else {
            return Err(format!("dismissed_concerns[{idx}] must be a JSON object."));
        };
        let has_desc = obj
            .get("description")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_desc {
            return Err(format!(
                "dismissed_concerns[{idx}] must have a non-empty 'description' string."
            ));
        }
        let has_reasoning = obj
            .get("reasoning")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_reasoning {
            return Err(format!(
                "dismissed_concerns[{idx}] must have a non-empty 'reasoning' string."
            ));
        }
        if !has_valid_proof_location(dismissed) {
            return Err(format!(
                "dismissed_concerns[{idx}] must include at least one entry in 'locations' with non-empty 'file', 'function_or_symbol', and verbatim disproving 'code_snippet' proving the candidate concern cannot occur. If you do not have concrete code proof, move the candidate issue to 'concerns' instead."
            ));
        }
    }

    Ok(())
}

fn format_concerns_feedback(violation: &str) -> String {
    format!(
        "\n\nPrevious attempt was rejected: {}. You MUST return ONLY a JSON object containing 'concerns' (each with 'type', non-empty 'description' and 'reasoning' strings, a boolean 'preexisting', and a 'locations' array) and 'dismissed_concerns' (each with 'type', non-empty 'description' and 'reasoning' strings, and a 'locations' array) arrays. If there are no concerns and no dismissed concerns, return `{{\"concerns\": [], \"dismissed_concerns\": []}}`.",
        violation
    )
}

// ---------------------------------------------------------------------------
// Stage Builders
// ---------------------------------------------------------------------------

pub fn prescreen_stage() -> Stage<UserspacePatchReviewState, PrescreenOutput> {
    prescreen_stage_with_effort(0.0, None)
}

pub fn prescreen_stage_with_effort(
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<UserspacePatchReviewState, PrescreenOutput> {
    Stage::builder("pre-screen")
        .system_prompt(PromptTemplate::<UserspacePatchReviewState>::new(
            "You are an AI assistant preparing a review of a patch to a userspace C or C++ project.\nReview the provided Patch and select all potentially relevant library and pattern guides from the index below.\nCRITICAL BIAS RULE: You MUST err on the side of inclusion. Only exclude a guide if it is 100% irrelevant to the modified code. If there is any doubt, include the file.\n\nYou MUST respond with ONLY a JSON object, no other text. Example:\n```json\n{\"selected_prompts\": [\"glib.md\", \"gobject.md\"]}\n```",
        ))
        .user_prompt(
            PromptTemplate::<UserspacePatchReviewState>::new(
                "<subsystem_guide_index>\n@include(\"subsystem/subsystem.md\")\n</subsystem_guide_index>\n\n<patch>\n{{target_commit_diff}}\n</patch>",
            )
            .with_var("target_commit_diff", |s: &UserspacePatchReviewState| {
                s.target_commit_diff.clone()
            })
            .include_file("subsystem/subsystem.md"),
        )
        .output_format(OutputFormat::json_with_schema(json!({
            "type": "object",
            "properties": {
                "selected_prompts": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["selected_prompts"]
        })))
        .policy(StagePolicy {
            tools: ToolScope::None,
            max_turns: 1,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::lightweight_stage_effort),
            ..Default::default()
        })
        .skip_if(|s| s.manual_stages.is_some())
        .reduce(|state, out: PrescreenOutput| {
            let prompts: Vec<String> = out
                .selected_prompts
                .into_iter()
                .map(|name| normalize_guide_name(&name).to_string())
                .filter(|name| !is_stage_exclusive_guide(name))
                .filter(|name| sanitize_guide_name(name))
                .collect();
            state.selected_guides = prompts;
        })
        .build()
}

pub fn validate_userspace_planning_output_with_effort(
    out: &PlanningOutput,
    _state: &UserspacePatchReviewState,
) -> Result<(), String> {
    validate_planning_stage_efforts(analysis_stage_by_name, out)
}

pub fn planning_stage() -> Stage<UserspacePatchReviewState, PlanningOutput> {
    planning_stage_with_effort(0.0, None)
}

pub fn planning_stage_with_effort(
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<UserspacePatchReviewState, PlanningOutput> {
    let dynamic_effort = effort == Some(crate::ai::EffortMode::Dynamic);
    let optional_stages: Vec<&'static str> = ANALYSIS_STAGES
        .iter()
        .filter(|s| s.optional)
        .map(|s| s.name)
        .collect();
    let optional_list = optional_stages.join(", ");

    let (sys_prompt, output_format) = if dynamic_effort {
        let stage_efforts_schema = build_stage_efforts_schema(ANALYSIS_STAGES);
        (
            format!(
                "You are an AI assistant planning a review of a patch to a userspace C or C++ project.\nThe core stages (goal, implementation, execution-flow) always run.\n1. Select which optional specialized stages should also run based on the patch contents.\nAvailable optional stages: [{optional_list}]\nInclude an optional stage when the patch touches or interacts with that domain; omit stages that are clearly irrelevant to the modified code.\n\n2. Select the reasoning effort (\"medium\" or \"high\") in stage_efforts for each stage that will run (goal, implementation, execution-flow, plus each stage selected in relevant_stages):\n- Use \"high\" for stages whose domain is central to the patch or where complex control flow, subtle error/teardown paths, concurrency, or non-trivial invariants must be verified.\n- Use \"medium\" for stages performing a routine check on a straightforward aspect of the patch.\n\nRespond with ONLY a JSON object. Example:\n```json\n{{\"relevant_stages\": [\"resources\", \"platform\"], \"stage_efforts\": {{\"goal\": \"medium\", \"implementation\": \"high\", \"execution-flow\": \"high\", \"resources\": \"high\", \"platform\": \"medium\"}}}}\n```"
            ),
            OutputFormat::json_with_schema(json!({
                "type": "object",
                "properties": {
                    "relevant_stages": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "stage_efforts": stage_efforts_schema
                },
                "required": ["relevant_stages", "stage_efforts"]
            }))
            .with_validator(validate_userspace_planning_output_with_effort)
            .with_feedback_formatter(format_planning_feedback),
        )
    } else {
        (
            format!(
                "You are an AI assistant planning a review of a patch to a userspace C or C++ project.\nThe core stages (goal, implementation, execution-flow) always run.\nSelect which optional specialized stages should also run based on the patch contents.\nAvailable optional stages: [{optional_list}]\n\nCRITICAL BIAS RULE: Err on the side of inclusion. Include any stage whose domain could plausibly be affected by the patch.\n\nRespond with ONLY a JSON object listing the relevant optional stages. Example:\n```json\n{{\"relevant_stages\": [\"resources\", \"platform\"]}}\n```"
            ),
            OutputFormat::json_with_schema(json!({
                "type": "object",
                "properties": {
                    "relevant_stages": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["relevant_stages"]
            })),
        )
    };

    Stage::builder("planning")
        .system_prompt(PromptTemplate::<UserspacePatchReviewState>::new(sys_prompt))
        .user_prompt(
            PromptTemplate::<UserspacePatchReviewState>::new(
                "<patch>\n{{target_commit_diff}}\n</patch>",
            )
            .with_var("target_commit_diff", |s: &UserspacePatchReviewState| {
                s.target_commit_diff.clone()
            }),
        )
        .output_format(output_format)
        .policy(StagePolicy {
            tools: ToolScope::None,
            max_turns: 1,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::lightweight_stage_effort),
            ..Default::default()
        })
        .skip_if(|s| s.manual_stages.is_some())
        .reduce(move |state, out: PlanningOutput| {
            apply_planning_stage_output(
                ANALYSIS_STAGES,
                analysis_stage_by_name,
                dynamic_effort,
                out,
                &mut state.planned_stages,
                &mut state.planned_efforts,
            );
        })
        .build()
}

fn analysis_stage(
    def: &'static AnalysisStage,
    max_turns: usize,
    temperature: f32,
    reasoning_effort: Option<crate::ai::ReasoningEffort>,
) -> Box<dyn ExecutableStage<UserspacePatchReviewState>> {
    let series_context = series_context_placeholder(def.wants_series_context);
    let mut user_template = PromptTemplate::<UserspacePatchReviewState>::new(format!(
        "{}{}\n\n{}",
        def.instruction, series_context, CONCERN_JSON_SCHEMA_EXAMPLE
    ));

    for guide in def.guides {
        user_template = user_template.include_file(*guide);
    }

    let user_template = with_series_context(user_template, def.wants_series_context);

    Box::new(
        Stage::builder(def.name)
            .system_prompt(userspace_system_prompt(def.uses_commit_log))
            .user_prompt(user_template)
            .output_format(
                OutputFormat::json()
                    .with_validator(validate_concerns_output)
                    .with_feedback_formatter(format_concerns_feedback),
            )
            .policy(StagePolicy {
                tools: ToolScope::All,
                max_turns,
                temperature,
                reasoning_effort,
                ..Default::default()
            })
            .reduce_with_outcome(
                move |state: &mut UserspacePatchReviewState,
                      out: StageConcernsOutput,
                      outcome: &crate::workflow::stage::StageOutcome| {
                    let prompts =
                        collect_stage_prompts(&state.selected_guides, def.guides, outcome);
                    append_stage_items_with_prompts(
                        &mut state.all_concerns,
                        &out.concerns,
                        def.name,
                        "General",
                        &prompts,
                    );
                    append_stage_dismissed_concerns_with_prompts(
                        &mut state.all_dismissed_concerns,
                        &out.dismissed_concerns,
                        def.name,
                        &prompts,
                    );
                },
            )
            .build(),
    )
}

pub fn resolve_analysis_stages_with_options(
    state: &UserspacePatchReviewState,
    max_turns: usize,
    temperature: f32,
) -> Vec<Box<dyn ExecutableStage<UserspacePatchReviewState>>> {
    let selected_stages: Vec<String> = if let Some(ref manual) = state.manual_stages {
        manual.clone()
    } else if !state.planned_stages.is_empty() {
        state.planned_stages.clone()
    } else {
        ANALYSIS_STAGES.iter().map(|d| d.name.to_string()).collect()
    };

    let mut stages = Vec::new();
    for name in selected_stages {
        match analysis_stage_by_name(&name) {
            Some(def) => {
                let effort = resolve_stage_effort(state, def.name);
                stages.push(analysis_stage(def, max_turns, temperature, effort));
            }
            None => tracing::warn!("Ignoring unknown userspace review stage {:?}", name),
        }
    }
    stages
}

pub fn verification_stage(
    max_turns: usize,
    temperature: f32,
) -> Stage<UserspacePatchReviewState, VerificationOutput> {
    verification_stage_with_effort(max_turns, temperature, None)
}

pub fn verification_stage_with_effort(
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<UserspacePatchReviewState, VerificationOutput> {
    let series_context = series_context_placeholder(VERIFICATION.wants_series_context);
    let user_template = with_series_context(
        PromptTemplate::<UserspacePatchReviewState>::new(format!(
            r#"{STAGE_VERIFICATION_INSTRUCTION}

<false_positive_guide>
@include("false-positive-guide.md")
</false_positive_guide>

<severity_guidelines>
@include("severity.md")
</severity_guidelines>@includes

CRITICAL REVIEW DIRECTIVE: To dismiss a concern as a false positive, you must have concrete evidence in the code that proves the concern is invalid. Never drop a raised concern into 'dismissed_concerns' in this stage: every consolidated concern must be placed either in 'findings' (if well-justified with concrete code proof and no competing dismissal) or in 'hard_cases' (if contested, speculative, or requiring tool verification). Also inspect every standalone dismissed_concern: if it dismissed a plausible bug using an unverified assumption, promote it into 'hard_cases' with '"signal_reason": "speculative_dismissal"'.{series_context}

Aggregated Concerns:
{{{{aggregated_concerns}}}}

Aggregated Dismissed Concerns:
{{{{aggregated_dismissed_concerns}}}}

Return ONLY a JSON object with 'findings', 'hard_cases', and 'dismissed_concerns' arrays.
- LINEAGE REQUIREMENT ('source_ids'): Every item in Aggregated Concerns has an "id" ("C1", "C2", ...) and every item in Aggregated Dismissed Concerns has an "id" ("D1", "D2", ...). Every object in 'findings', 'hard_cases', and 'dismissed_concerns' MUST include a non-empty "source_ids" array of strings listing the exact input "C*" and/or "D*" IDs consolidated into that output item. Every input "C*" and "D*" ID MUST appear in at least one output item's "source_ids". Category 1a 'findings' may ONLY reference "C*" IDs (any contested "C*" + "D*" or promoted "D*" must be routed to 'hard_cases'), and Category 1b 'dismissed_concerns' may ONLY reference "D*" IDs in "source_ids" (never "C*" IDs).
- Each object in 'findings' (Category 1a: Well-Justified Concerns) MUST use the keys: "source_ids" (non-empty array of "C*" IDs only), "problem" (a short naming string under 80 characters starting with a short component prefix taken from the touched file or module, like 'elanpress:', 'daemon:', 'backend/drm:', NEVER using backquotes), "severity" ("Low", "Medium", "High", "Critical", or "Unknown"), "severity_explanation" (detailed reasoning and proof), "preexisting" (boolean), and "locations" (array of objects with file, function_or_symbol, line, code_snippet, and why_this_location_matters), and may include "stages" (array of stage names) and "prompts" (array of prompt files).
- Each object in 'hard_cases' (Category 2: Speculative or Contested) MUST use the keys: "source_ids" (non-empty array of input IDs), "type", "description", "estimated_severity" ("Low", "Medium", "High", "Critical", or "Unknown"), "signal_reason" ("mixed_signals", "speculative_concern", "speculative_dismissal", "series_interaction", or "other"), "concern_arguments" (consolidated arguments for why the bug can occur), "dismissal_arguments" (consolidated arguments/snippets from any competing or standalone dismissal, or "" if none), "verification_question" (the specific code question post-verification must answer with tools), "preexisting" (boolean), and "locations" (array of location objects), and may include "stages" and "prompts".
- Each object in 'dismissed_concerns' (Category 1b: Well-Justified Dismissals) MUST use the keys: "source_ids" (non-empty array of "D*" IDs only), "type", "description", "reasoning", and "locations", and may include "stages" and "prompts"."#
        ))
        .include_file("false-positive-guide.md")
        .include_file("severity.md")
        .include_files_from_state(|s: &UserspacePatchReviewState| {
            extra_prompt_paths_for_state(s, analysis_stage_by_name)
        }),
        VERIFICATION.wants_series_context,
    )
    .with_var("aggregated_concerns", |s: &UserspacePatchReviewState| {
        serde_json::to_string_pretty(&s.all_concerns).unwrap_or_default()
    })
    .with_var(
        "aggregated_dismissed_concerns",
        |s: &UserspacePatchReviewState| {
            serde_json::to_string_pretty(&s.all_dismissed_concerns).unwrap_or_default()
        },
    );

    Stage::builder(VERIFICATION.name)
        .system_prompt(userspace_system_prompt(true))
        .user_prompt(user_template)
        .output_format(
            OutputFormat::json()
                .with_validator(validate_verification_stage_output)
                .with_feedback_formatter(format_verification_stage_feedback),
        )
        .policy(StagePolicy {
            tools: ToolScope::All,
            max_turns,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::verification_stage_effort),
            ..Default::default()
        })
        .skip_if(|s| s.all_concerns.is_empty() && s.all_dismissed_concerns.is_empty())
        .reduce_with_outcome(|state, out: VerificationOutput, outcome| {
            apply_verification_stage_output(state, out, outcome, analysis_stage_by_name);
        })
        .build()
}

pub fn post_verification_stage(
    stage_name: &'static str,
    batch: Vec<Value>,
    max_turns: usize,
    temperature: f32,
) -> Stage<UserspacePatchReviewState, PostVerificationOutput> {
    post_verification_stage_with_effort(stage_name, batch, max_turns, temperature, None)
}

pub fn post_verification_stage_with_effort(
    stage_name: &'static str,
    batch: Vec<Value>,
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<UserspacePatchReviewState, PostVerificationOutput> {
    let candidate_json = serde_json::to_string_pretty(&batch).unwrap_or_default();
    let batch_for_prompts = batch.clone();
    let batch_for_validate = batch.clone();
    let batch_for_reduce = batch;
    let series_context = series_context_placeholder(POST_VERIFICATION.wants_series_context);
    let user_template = with_series_context(
        PromptTemplate::<UserspacePatchReviewState>::new(format!(
            r#"{STAGE_POST_VERIFICATION_INSTRUCTION}

<false_positive_guide>
@include("false-positive-guide.md")
</false_positive_guide>

<severity_guidelines>
@include("severity.md")
</severity_guidelines>@includes

CRITICAL REVIEW DIRECTIVE: To dismiss a code-behavior candidate issue as a false positive, you must find concrete evidence in the code that proves the issue is invalid and quote that disproving code in `dismissed_concerns[].locations` (for build failures dismissed under rule 1, carry forward the candidate's target location in `locations`). If you cannot find concrete proof of safety, you must validate and report the finding in `findings`.{series_context}

Candidate Hard Case(s) to Verify:
{{{{candidate_hard_cases}}}}

Return ONLY a JSON object with 'findings' and 'dismissed_concerns' arrays. Every candidate in this batch MUST be accounted for in either 'findings' (if validated) or 'dismissed_concerns' (if disproved by concrete code or excluded by rule 1; never return both empty arrays).
- LINEAGE REQUIREMENT ('source_ids'): Every candidate hard case in this batch has an "id" ("H1", "H2", ...). Every object in 'findings' and 'dismissed_concerns' MUST include a non-empty "source_ids" array of strings listing the exact candidate "H*" ID(s) from this batch that it resolves, and every candidate "H*" ID in this batch MUST be accounted for in at least one output item's "source_ids".
- Each object in 'findings' MUST use: "source_ids" (non-empty array of batch "H*" IDs), "problem" (a short naming string under 80 characters starting with a short component prefix taken from the touched file or module, like 'elanpress:', 'daemon:', 'backend/drm:', NEVER using backquotes), "severity" (Low, Medium, High, Critical, or Unknown), "severity_explanation" (detailed reasoning and proof), "preexisting" (boolean), "locations" (array of objects with file, function_or_symbol, line, code_snippet, and why_this_location_matters).
- Each object in 'dismissed_concerns' MUST use: "source_ids" (non-empty array of batch "H*" IDs), "description" (the candidate issue that was disproved), "reasoning" (step-by-step explanation of how the inspected code or policy rule disproves the candidate), and "locations" (a non-empty array of objects with file, function_or_symbol, line, code_snippet, and why_this_location_matters, quoting the verbatim disproving code or target snippet)."#
        ))
        .include_file("false-positive-guide.md")
        .include_file("severity.md")
        .include_files_from_state(move |s: &UserspacePatchReviewState| {
            extra_prompt_paths_for_items(&s.selected_guides, &batch_for_prompts, analysis_stage_by_name)
        }),
        POST_VERIFICATION.wants_series_context,
    )
    .with_var("candidate_hard_cases", move |_: &UserspacePatchReviewState| {
        candidate_json.clone()
    });

    Stage::builder(stage_name)
        .system_prompt(userspace_system_prompt(true))
        .user_prompt(user_template)
        .output_format(
            OutputFormat::json()
                .with_validator(
                    move |out: &PostVerificationOutput, _: &UserspacePatchReviewState| {
                        validate_post_verification_batch_items(out, &batch_for_validate)
                    },
                )
                .with_feedback_formatter(format_post_verification_feedback),
        )
        .policy(StagePolicy {
            tools: ToolScope::All,
            max_turns,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::verification_stage_effort),
            ..Default::default()
        })
        .reduce_with_outcome(move |state, out: PostVerificationOutput, outcome| {
            apply_post_verification_stage_output(
                state,
                stage_name,
                &batch_for_reduce,
                out,
                outcome,
                analysis_stage_by_name,
            );
        })
        .build()
}

pub fn post_verification_stage_for_batch(
    stage_name: &'static str,
    batch: Vec<Value>,
    max_turns: usize,
    temperature: f32,
) -> Box<dyn ExecutableStage<UserspacePatchReviewState>> {
    Box::new(post_verification_stage_with_effort(
        stage_name,
        batch,
        max_turns,
        temperature,
        None,
    ))
}

pub fn resolve_post_verification_stages_with_options(
    state: &UserspacePatchReviewState,
    max_turns: usize,
    temperature: f32,
) -> Vec<Box<dyn ExecutableStage<UserspacePatchReviewState>>> {
    let effort = state.effort;
    let batches = batch_hard_cases_by_severity(&state.hard_cases);
    batches
        .into_iter()
        .enumerate()
        .map(|(idx, batch)| {
            let stage_name = POST_VERIFICATION_STAGE_NAMES[idx];
            Box::new(post_verification_stage_with_effort(
                stage_name,
                batch,
                max_turns,
                temperature,
                effort,
            )) as Box<dyn ExecutableStage<UserspacePatchReviewState>>
        })
        .collect()
}

pub fn report_stage(
    max_turns: usize,
    temperature: f32,
) -> Stage<UserspacePatchReviewState, String> {
    report_stage_with_effort(max_turns, temperature, None)
}

pub fn report_stage_with_effort(
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<UserspacePatchReviewState, String> {
    Stage::builder(REPORT.name)
        .system_prompt(userspace_system_prompt(true))
        .user_prompt(
            PromptTemplate::<UserspacePatchReviewState>::new(format!(
                r#"{STAGE_REPORT_INSTRUCTION}

<report_template>
@include("inline-template.md")
</report_template>

Findings:
{{{{findings}}}}

Return raw text output, not JSON."#
            ))
            .include_file("inline-template.md")
            .with_var("findings", |s: &UserspacePatchReviewState| {
                serde_json::to_string_pretty(&s.findings).unwrap_or_default()
            }),
        )
        .output_format(OutputFormat::text_with_validator(
            validate_inline_format,
            format_inline_feedback,
        ))
        .policy(StagePolicy {
            tools: ToolScope::All,
            max_turns,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::lightweight_stage_effort),
            recitation_policy: RecitationPolicy::FallbackToFreeForm {
                reminder: "Your previous response was blocked for quoting the patch diff verbatim. Do not quote code or diff lines with '>'. Start with the commit and Author headers, write the summary, and describe each finding by file and function name in plain prose.".to_string(),
            },
            ..Default::default()
        })
        .skip_if(|s| s.skip_report)
        .reduce(|state, out: String| {
            state.review_inline = out;
        })
        .build()
}

// ---------------------------------------------------------------------------
// Complete Userspace Review Workflow Graph
// ---------------------------------------------------------------------------

pub fn build_userspace_patch_review_workflow() -> Workflow<UserspacePatchReviewState> {
    build_userspace_patch_review_workflow_with_options(20, 0.0)
}

pub fn build_userspace_patch_review_workflow_with_options(
    max_turns: usize,
    temperature: f32,
) -> Workflow<UserspacePatchReviewState> {
    build_userspace_patch_review_workflow_with_effort(max_turns, temperature, None)
}

pub fn build_userspace_patch_review_workflow_with_effort(
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Workflow<UserspacePatchReviewState> {
    Workflow::builder("userspace_patch_review")
        .stage(prescreen_stage_with_effort(temperature, effort))
        .dynamic_parallel(
            planning_stage_with_effort(temperature, effort),
            move |state| resolve_analysis_stages_with_options(state, max_turns, temperature),
            ParallelPolicy::BestEffort,
        )
        .early_exit_if(
            |s| s.all_concerns.is_empty() && s.all_dismissed_concerns.is_empty(),
            "No concerns or dismissed concerns raised in initial analysis stages",
        )
        .dynamic_parallel(
            verification_stage_with_effort(max_turns, temperature, effort),
            move |state| {
                resolve_post_verification_stages_with_options(state, max_turns, temperature)
            },
            ParallelPolicy::BestEffort,
        )
        .early_exit_if(
            |s| s.findings.is_empty(),
            "No findings validated in verification stages",
        )
        .stage(report_stage_with_effort(max_turns, temperature, effort))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_names_are_unique_and_resolvable() {
        let mut seen = std::collections::BTreeSet::new();
        for def in ANALYSIS_STAGES {
            assert!(seen.insert(def.name), "duplicate stage name {}", def.name);
            assert!(is_known_stage(def.name));
            assert!(std::ptr::eq(analysis_stage_by_name(def.name).unwrap(), def));
        }
        for stage in CONSOLIDATION_STAGES {
            assert!(is_known_stage(stage.name));
        }
    }

    #[test]
    fn test_core_stages_always_run() {
        for name in ["goal", "implementation", "execution-flow"] {
            assert!(!analysis_stage_by_name(name).unwrap().optional, "{name}");
        }
    }

    #[test]
    fn test_stage_guides_exist_in_the_prompt_tree() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("projects/userspace/prompts");
        for def in ANALYSIS_STAGES {
            for guide in def.guides {
                assert!(
                    root.join(guide).is_file(),
                    "{} names missing guide {guide}",
                    def.name
                );
            }
        }
        for file in [
            "review-core.md",
            "subsystem/subsystem.md",
            "false-positive-guide.md",
            "severity.md",
            "inline-template.md",
        ] {
            assert!(root.join(file).is_file(), "missing {file}");
        }
    }

    #[test]
    fn test_indexed_guides_exist() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("projects/userspace/prompts");
        let index = std::fs::read_to_string(root.join("subsystem/subsystem.md")).unwrap();
        let mut found = 0;
        for line in index.lines().filter(|l| l.starts_with('|')) {
            let Some(file) = line.trim_end_matches('|').rsplit('|').next().map(str::trim) else {
                continue;
            };
            if !file.ends_with(".md") {
                continue;
            }
            found += 1;
            assert!(
                root.join("subsystem").join(file).is_file()
                    || root.join("patterns").join(file).is_file(),
                "index names missing guide {file}"
            );
        }
        assert!(found > 0);
    }

    #[test]
    fn test_build_userspace_patch_review_workflow() {
        let wf = build_userspace_patch_review_workflow();
        assert_eq!(wf.name, "userspace_patch_review");
        // pre-screen, planning fan-out, early exit, verification fan-out,
        // early exit, report.
        assert_eq!(wf.steps.len(), 6);
    }
}
