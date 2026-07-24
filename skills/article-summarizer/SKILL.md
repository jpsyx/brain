---
name: article-summarizer
description: Use when the user wants an article, paper, study, blog post, or long web page distilled into a clean, faithful summary — "summarize this article", "give me the gist of this paper", "TL;DR this link". Produces a structured summary (executive summary, key points, findings) suitable for filing into a knowledge base. Other skills call this one rather than re-deriving the method.
---

# article-summarizer

Distill a single source into a clean, faithful, structured summary. This is the
generic "how to summarize" skill; other skills (e.g. second-brain when filing
into resources, or a personal reference-manager plugin) call it rather than
re-deriving the method.

Before you begin, load the user's personalization so framing fits them: run
`brain personalize show` and note their `role`/`works_for` (they may be unset —
then keep the framing neutral). The generic rule is simply to summarize
carefully and faithfully; who it's for is personalization, not a hardcoded
identity.

## Get the source first

- If given a URL, fetch it. If given a file, read it. Never summarize from the
  title, a snippet, or memory alone.
- If you cannot get the actual content, **stop and say so** — do not produce a
  summary from what you happen to know about the source (see Red flags).
- If only an abstract/metadata is available, you may proceed, but **state
  explicitly** that the summary is based on the abstract only.

## Voice

Adopt the voice of a careful researcher synthesizing a source: precise,
discipline-appropriate vocabulary, foreground methodology and evidence. Avoid
generic "this paper discusses…" openings and avoid editorializing. Preserve the
source's own hedges and terminology.

## Output structure

Use **exactly** this Markdown structure — no preamble, no closing remarks, no
meta-commentary:

```markdown
## Summary

### Executive summary

[2 sentences capturing the core question and the headline finding.]

### Key points

- [Context, motivation, or gap being addressed.]
- [Methodology — design, data sources, sample size, analytical approach.]
- [Primary result, with effect sizes or statistics where reported.]
- [Secondary result or nuance, if applicable.]
- [Implications or limitations, if notable.]

### Conclusion / key findings

- [Most important takeaway or contribution.]
- [Practical or policy implication, if applicable.]
- [Limitation or direction for future work, if applicable.]
```

`## Summary` is the canonical container a knowledge base can share (e.g. a
`notes.md` filed alongside the source); sub-headings are H3. Omit any bullet the
source does not support rather than padding to hit the count — the last two Key
points and the second/third Conclusion bullets are optional. For short web
articles, blog posts, or opinion pieces, the summary may be briefer; match depth
to the source.

## Rules

- **Never fabricate** sample sizes, p-values, effect sizes, citations, author
  names, or conclusions. If a number is not in the retrieved text, do not invent
  one.
- **Hedge appropriately**: attribute uncertain or abstract-only claims ("the
  authors report…", "the abstract states…").
- **Never summarize from training data.** No source in hand → stop and say so.
- **Preserve the source's discipline.** Match the field's terminology and
  conventions (medicine, economics, ML, policy, etc.).
- **Reuse, don't redo.** If the source has an author-written abstract or a
  high-quality existing summary, use it verbatim under `## Summary` rather than
  producing a new one.

## When to skip summarizing

Skip when the *whole document is the value* and a 5-bullet distillation would
lose what it was saved for: reference docs, cheat sheets, API docs; datasets and
spreadsheets; code and config; images/diagrams without substantive prose; short
notes the user wrote themselves. Summarize when the source is a paper, study,
news article, opinion piece, blog post, long-form essay, or other prose-heavy
artifact.

## Red flags — stop and reconsider

- You're about to write a statistic the retrieved text doesn't contain.
- You're filling in methodology details not in the source.
- You can't find the source but you "know" it from elsewhere.
- You were asked to summarize a document you haven't actually read.

All mean: stop, retrieve the source (or ask the user), then proceed.

## Output handling

Return the summary as clean Markdown. Do not file it anywhere yourself — the
calling skill (or the user) decides where it lands. Keep it tight: shorter than
the source, losing none of its load-bearing content.
