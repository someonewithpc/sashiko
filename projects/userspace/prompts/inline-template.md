Produce a report of the findings in this format. It is read as a reply to a
patch on a mailing list or as a comment on a merge request.

## Rules

- Plain text only. No markdown, no code fences, no headings.
- Keep long lines from the quoted diff as they are. Wrap your own text at 78
  characters.
- Never include findings that were dismissed as false positives.
- Include every finding you were given, even if its details say a later
  commit fixes it. Mention that fix in the comment.
- Write factual, undramatic questions about the code, not accusations, and
  do not address the author. Write "Can this free the device twice?", not
  "You free the device twice."
- Vary how questions start.
- Name the specific thing: "Does this leak the GBytes returned by
  read_frame()?", not "Is there a resource leak?".
- Never use all caps except when quoting code.
- Do not mention line numbers. They refer to this review's checkout and mean
  nothing to the reader. Quote a short code snippet or name the function
  and call chain (foo()->bar()) instead.
- Keep paragraphs short. Put a blank line between a group of facts and the
  question that follows them.
- Do not explain why a typo matters; point it out.
- A subjective point that is not a bug starts with "This isn't a bug, but".
- For a finding marked preexisting, start the comment with "This wasn't
  introduced by this patch, but".

## Structure

1. The commit header as git show prints it:
   commit <full sha>
   Author: <name> <email>
2. The one-line subject of the commit.
3. A summary of the commit in at most three sentences.
4. The parts of the diff that the findings refer to, quoted with "> " at the
   start of every line, exactly as in the commit. Get the diff with git_show;
   do not rebuild it from memory.
5. After the quoted lines each finding is about, a line with the severity in
   square brackets, then the comment, without "> ":

[Severity: High]
Can this callback run after dev is freed? ...

Keep the diff headers of files you quote from. Replace snipped parts with
"[ ... ]". Snip whole files, hunks and functions the findings do not need,
and anything after the last comment.

For a finding about the commit message, quote the relevant part of the
message first, then the comment.

End the report with a blank line.

## Sample

commit 3f2a9c1d8e4b7a6f5c0e1d2b3a4f5e6d7c8b9a0f
Author: Jane Doe <jane@example.org>

backend: re-read connectors on hotplug

This commit re-reads every connector when a hotplug uevent arrives and
drops outputs whose connector disappeared.

> diff --git a/src/backend/drm/DRM.cpp b/src/backend/drm/DRM.cpp
> --- a/src/backend/drm/DRM.cpp
> +++ b/src/backend/drm/DRM.cpp

[ ... ]

> +    for (auto& c : connectors) {
> +        if (!seen.contains(c->id))
> +            connectors.erase(std::ranges::find(connectors, c));
> +    }

[Severity: High]
Does erasing from connectors inside this range-for invalidate the loop
iterator?

The next iteration reads the element after the erased one through an
iterator that erase() has already invalidated.

