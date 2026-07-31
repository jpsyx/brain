---
name: brain-knowledge-capture
description: Use when the user has learned something durable in a conversation and wants it saved into their brain — "capture this", "save what I just learned", "add this to my brain", "write this up as a note", or any time a session produced a reusable insight worth keeping. Distills the knowledge into a clean note and files it via the second-brain PARA rules.
---

# brain-knowledge-capture

Turn knowledge produced in a conversation (an explanation, a decision, a
debugging insight, a mental model the user just built) into a clean,
reusable note in the user's brain. This is the "harvest what I just
learned" skill.

Throughout, `<brain>` is the user's brain root — the directory `brain config
get root` returns (default `~/brain`) — and `~/.agents/skills/second-brain/`
is where `brain skills sync` installs the `/second-brain` skill (with its
`reindex.py` and `cleanup.sh`). Both resolve without hardcoding a personal path.

It is a **capture specialist**, not a filing-system authority. It owns
*what is worth keeping* and *how to write it down well*, and it
**delegates where the note goes** to the PARA rules in
[`/second-brain`](../second-brain/SKILL.md). Tasks that fall out of the
knowledge go to [`/todo`](../todo/SKILL.md). Don't re-derive placement
rules here; defer to second-brain.

## Personal-assistant mode

Touching the brain puts you in personal/executive-assistant mode. Load who
you're assisting: run `brain personalize show` and honor their `role`/`works_for`
(both may be unset — then stay neutral). **Top priority: save the user's time.**
Be blunt and distill hard. Make the obvious calls yourself (topic, filename,
whether a point is durable) and state them. Ask only when second-brain's rules
require it (a new subdirectory, a genuinely ambiguous bucket) or when you
can't judge whether something is durable enough to keep. Batch any
questions.

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
with little distillation. That's second-brain's
["Add a resource"](../second-brain/SKILL.md) command. This skill is for
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

3. **Classify the bucket** with second-brain's
   [decision flow](../second-brain/SKILL.md). General learnings default
   to **`resources/<topic>/`**. Use a **project** folder only when the
   knowledge was produced *for* an active project and is specific to it;
   an **area** only when it's about an ongoing responsibility. Torn
   between "tied to this project" and "generally reusable"? Prefer
   `resources/` so future work can pull it in (it's an Intermediate
   Packet; see second-brain).

4. **Find the home recursively, and dedupe.** Before creating anything,
   search for a note this belongs in or beside:
   ```
   fd -t d <topic-or-synonyms> <brain>/resources
   rg -il '<key terms>' <brain>/resources
   ```
   - If a close match exists, **extend or link it** instead of
     duplicating. Append only when the material clearly belongs in the
     same narrow subject; otherwise write a sibling note and cross-link.
   - Nest under the most natural existing parent
     (`resources/software-engineering/frontend/`, not a new top-level
     `resources/react/`).

5. **Confirm placement only where second-brain requires it.** A new
   *file* in an existing folder: place it and state where. A topic you
   picked yourself or a folder you're reusing: state your choice and
   proceed. A new **subdirectory** (top-level or nested): describe the
   path and parent, and get explicit confirmation before `mkdir`.

6. **Write the note** with the template below. Lower-kebab-case filename
   describing the subject. Every brain file you mention gets a relative
   markdown link. Close with a `See also` section when the note has
   genuinely related neighbours (see
   [the rule below](#add-a-see-also-section-when-theres-something-worth-linking)).

7. **Reindex, then clean up.** If you wrote a `.METADATA.json` under
   `resources/` or `projects/`, run the matching
   [reindex](../second-brain/SKILL.md):
   ```
   python3 ~/.agents/skills/second-brain/reindex.py --resources
   ```
   A plain prose note in an existing topic folder needs no reindex (the
   lookup CSVs track reference-manager items, not every note). Always
   finish with the byproduct cleanup:
   ```
   bash ~/.agents/skills/second-brain/cleanup.sh
   ```

8. **Reply** with a one-line summary of what you captured and a relative
   markdown link to the note.

## Note template

```
# <Subject — what the reader learns>

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

- [related note](relative/path/to/note.md) — one line on why it's relevant
```

Adapt the sections to the content. A small insight can be just the H1,
the framing, and one principle. Don't pad to fill the template.

## Add a "See also" section when there's something worth linking

**Before finishing a note, search the brain for genuinely related
material and, if you find any, cross-link it in a `See also` section**
(notes, files, or directories). The brain's value compounds through
connections, so a note with real neighbours should join the graph.

This is **not** mandatory. The gate is relevance, not obligation:

- Search for neighbours first (`fd`/`rg` over `<brain>` for the topic and
  its synonyms, plus the sibling files in the destination folder). Don't
  skip the search; that's how you find the links worth making.
- **Link only what truly relates.** A reader following the link should
  land on something that meaningfully connects, not a word-match. A few
  strong links beat a long list of weak ones.
- **If an honest search finds nothing relevant, omit the section.** Don't
  invent tenuous links or pad it to look connected. No `See also` is the
  right outcome for a note with no real neighbours.
- When you do link: notes, non-markdown files, and directories are all
  valid targets (a related PDF, a dataset, a whole topic folder), always
  via **relative markdown links** (a directory link ends in `/`). The
  shape to aim for: a note that links a sibling note and an adjacent
  directory, each with a one-line reason for the link.

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
| Bare path or filename when referencing a note                 | Use a relative markdown link so the user can jump to it.            |
| Skipping the neighbour search, so real cross-links get missed | Always `fd`/`rg` for related material; add `See also` when you find genuinely relevant notes/files/dirs. |
| Padding `See also` with tenuous links to look connected       | Relevance is the gate. Link only what truly relates; if nothing does, omit the section. |
| Creating a new subdirectory without confirming                | New *files* are fine to place; new *subdirs* need confirmation.     |
| Forgetting cleanup after touching the brain                   | End every run with `bash ~/.agents/skills/second-brain/cleanup.sh`. |
