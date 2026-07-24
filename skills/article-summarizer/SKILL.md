---
name: article-summarizer
description: Use when the user wants an article, paper, blog post, or long web page distilled into a clean, faithful summary — "summarize this article", "give me the gist of this paper", "TL;DR this link". Produces a structured note (thesis, key points, evidence, caveats) suitable for filing into a knowledge base.
---

# article-summarizer

Distill a single article into a clean, faithful summary. This is the generic
"how to summarize an article" skill; other skills (e.g. second-brain, and any
personal sync plugins) call it rather than re-deriving the method.

Before you begin, load the user's personalization so the summary is framed for
them: run `brain personalize show` and note their `role` / `works_for` (they may
be unset — then keep the framing neutral). The generic rule is simply to act as
a careful summarizer; who it's for is personalization, not a hardcoded identity.

## Steps

1. **Get the full text.** If given a URL, fetch it. If given a file, read it.
   Never summarize from the title or a snippet alone.
2. **Read for structure**, not just words: what is the central claim, what
   supports it, what does the author concede or leave open.
3. **Write the summary** as:
   - **Thesis** — one or two sentences: the single load-bearing claim.
   - **Key points** — 3–7 bullets, each a distinct idea, in the article's own
     logic (not your reordering).
   - **Evidence** — the concrete data, studies, or examples the argument rests on.
   - **Caveats / open questions** — limitations the author notes or that are
     visibly missing.
   - **Source** — title, author, outlet, date, and the URL.
4. **Be faithful.** Do not add claims the article does not make, and flag where
   you are inferring versus quoting. Preserve the author's hedges.

## Output

Return the summary as clean Markdown. Do not file it anywhere yourself — the
calling skill (or the user) decides where it lands. Keep it tight: a good
summary is shorter than the article and loses none of its load-bearing content.
