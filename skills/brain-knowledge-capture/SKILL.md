---
name: brain-knowledge-capture
description: Use when the user has learned something durable in a conversation and wants it saved into their brain - "capture this", "save what I just learned", "add this to my brain", "write this up as a note", or after accepting an offer to capture a reusable insight. Distills the knowledge into a clean note and files it via the organize PARA rules.
---

# brain-knowledge-capture

Turn knowledge produced in a conversation (an explanation, a decision, a
debugging insight, a mental model the user just built) into a clean,
reusable note in the user's brain. This is the "harvest what I just
learned" skill.

Own what is worth keeping and how to distill it. Delegate filing to
[`/organize`](../organize/SKILL.md); route resulting actions to
[`/todo`](../todo/SKILL.md). Use the selected workspace and effective actor
from the shared workspace context.

<a id="personal-assistant-mode"></a>

The shared workspace contract supplies assistant context and actor identity.

## When to use it

- **Explicit:** "capture this", "save this to my brain", "write this up
  as a note", "add what I just learned to resources",
  "/brain-knowledge-capture".
- **Proactive offer:** when a session clearly produced durable, reusable
  knowledge (a worked-out mental model, a design decision with rationale,
  a non-obvious root cause, a comparison the user said they "learned a
  lot from"), offer to capture it in one line. Offer, then act on a yes.
  Never capture unprompted.

Don't invoke this for saving an external artifact (a PDF, a URL, a file)
with little distillation. That's organize's
["Add a resource"](../organize/SKILL.md#add-a-resource) command. This skill is for
knowledge that lives in the *conversation* and needs writing down.

## What is worth capturing

The whole value of capture is the filter. Keep the signal, drop the
scaffolding.

| Capture (durable, reusable)                                  | Drop (ephemeral or rederivable)                              |
|--------------------------------------------------------------|--------------------------------------------------------------|
| A mental model or principle the user can reuse later         | Restating something already obvious or well-documented       |
| A design decision **and its rationale**                      | Project status, todos, "what we did next" (those are tasks)  |
| A non-obvious root cause and the fix's underlying reason     | A one-off fix with no transferable lesson                    |
| A comparison or decision matrix between approaches           | Blow-by-blow transcript or your own narration                |
| A minimal, reproducible code pattern                         | Large code dumps that belong in the repo, not a note         |
| A gotcha or footgun and *why* it happens                     | Secrets, tokens, credentials, anything sensitive             |

Litmus test: **"Will future-me, on a different project, be glad this is
written down?"** If no, don't capture it.

## The capture workflow

1. **Extract the durable knowledge.** Pull out the claims, models,
   decisions, and examples that pass the filter above. Ignore the
   back-and-forth.

   **Re-read through any mid-conversation clarification.** If the user
   narrowed a term partway (e.g. "by *context* I meant *context +
   reducer + dispatch*"), re-read the earlier points under the corrected
   meaning. A point that only existed to resolve a now-dissolved
   ambiguity is scaffolding; drop it. "*X vs Y was the wrong comparison*"
   usually means the durable note is just "*X′ vs Y*" stated plainly:
   keep the substantive differences, drop the meta-claim that the axis
   was wrong. When you can't tell whether a reframe is the insight or
   discarded scaffolding, ask.

2. **Distill into the user's voice.** Rewrite tight, in your own words,
   not as a transcript. Favor a clear principle plus a minimal example
   over long narration. **No fabrication:** capture only what was
   established. Record a hypothesis ("likely a bug, worth verifying") as
   a hypothesis, not a fact. Keep code minimal and runnable.

3. **Delegate placement to `/organize`.** Use its
   [placement rules](../organize/SKILL.md#placement). Search existing notes
   recursively and deduplicate: extend a note only for the same narrow
   subject; otherwise write a sibling and cross-link. A reusable general
   learning belongs in resources; project-specific learning stays with its
   active project. Honor an explicit destination and proceed with routine
   filing choices already authorized by the capture request.

4. **Resolve real ambiguity.** Ask only for unresolved consequential choices,
   destructive disposition, or a new configured namespace. Do not ask for
   approval of every new subdirectory.

5. **Check provenance and certainty.** Preserve the context and source of each
   durable claim, including unresolved hypotheses as hypotheses.

6. **Write the note** with the template below. Lower-kebab-case filename
   describing the subject. Use relative links inside the saved note for real related material
   discovered by organize, without inventing tenuous connections.

7. **Apply organize's [reindex decision](../organize/SKILL.md#reindex).**
   Plain unindexed prose needs no lookup rebuild.

8. **Report the result.** Describe the knowledge captured, its provenance,
   saved destination, and any tasks created or uncertainties retained.

## Note template

```
# <Subject - what the reader learns>

Source: Captured from a <session kind> on <YYYY-MM-DD>.
<one line of provenance: what prompted it>

<1–3 sentence framing: the core takeaway up front, so future-me gets the
point without reading the whole note.>

## <Key idea / principle>

<Tight prose. State the model or rule, and explain the *why*: the reason
is the reusable part.>

<a fenced lang block here: the smallest runnable example>

## <Decision matrix / tradeoffs>   (when the knowledge is a comparison)

<table or bulleted axes>

## Gotchas

<footguns, and *why* they happen, if any>

## See also   (only when there are genuinely related neighbours)

- [related note](relative/path/to/note.md) - one line on why it's relevant
```

Adapt the sections to the content. A small insight can be just the H1,
the framing, and one principle. Don't pad to fill the template.

<a id="add-a-see-also-section-when-theres-something-worth-linking"></a>

## Related material

Use [organize](../organize/SKILL.md#add-a-resource) for the neighbor search
and relevant cross-links; omit `See also` when there are no real neighbors.

## Provenance

These notes have no source URL like an article does, so the `Source:`
line carries provenance: the kind of session (design discussion,
debugging, pairing), the absolute date (`YYYY-MM-DD`), and the project or
context. When the knowledge ties to a tracked decision elsewhere (an
issue tracker, a PR, a doc), name or link it.

## Common mistakes

| Mistake                                                       | Fix                                                                 |
|---------------------------------------------------------------|---------------------------------------------------------------------|
| Capturing the transcript instead of the lesson                | Distill to the principle plus a minimal example. Drop the back-and-forth. |
| Saving project status / next steps as "knowledge"             | Those are tasks. Route to [`/todo`](../todo/SKILL.md), not a note.  |
| Duplicating a topic that already has a home                   | `fd`/`rg` first; extend or cross-link the existing note.            |
| New top-level `resources/<topic>/` when a nested home exists  | Search recursively; nest under the natural parent.                  |
| Recording a hypothesis as established fact                    | Mark uncertainty; capture only what was verified.                   |
| Headlining the resolution of a terminology mix-up             | Re-read through the user's later clarification; keep the clean "X′ vs Y", not "X vs Y was wrong". |
