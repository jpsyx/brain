---
name: contacts
description: Use when the user asks about a person or service provider in their contacts book — "what's Maria's number?", "who is my accountant?", "what's my plumber's email?", "add a contact", "update so-and-so's address" — or any read/write to the local contacts CSV at `resources/contacts/`. Handles add / edit / delete / search / list via a deterministic CLI.
---

# contacts

The brain keeps a **local contacts book** — the user's people and
service providers — at `<brain>/resources/contacts/`. This skill owns
looking people up and keeping that book clean. It is a sibling of
[`/second-brain`](../second-brain/SKILL.md) (which references it for
contact lookups) and shares its brain-mutation conventions.

Throughout, `<brain>` is the user's brain root (`brain config get root`,
default `~/brain`) and `~/.agents/skills/contacts/scripts/contacts.py` is
this skill's CLI as installed by `brain skills sync`.

> **Non-negotiables for every contacts request (read before answering):**
> 1. **Handle it through this skill — never freelance.** Do the lookup
>    with `contacts.py` (`find`/`get`/`list`), *not* by `cat`/`grep`/
>    hand-reading `contacts.csv`, and reach any configured fallback only
>    via the documented path (`contacts.py notion`), *not* by a raw guess
>    against another tool. The CLI is the only sanctioned path.
> 2. **Every lookup answer is a markdown table** — see
>    [Presenting lookup results](#presenting-lookup-results--always-a-table).
>    No exceptions, including a "quick" one-field answer like a single
>    phone number. Prose-only replies are a bug.
>
> If you catch yourself about to answer a contacts question without
> having run `contacts.py`, stop and restart through the CLI.

## Storage

The contacts book lives at `<brain>/resources/contacts/`:

- `contacts.csv` — one row per contact, keyed by a stable `id`
  (`C001`, `C002`, …).
- `contacts.config.json` — column list + an optional fallback pointer
  (see [Lookup precedence](#lookup-precedence--local-csv-first-then-fallback)).
  Named `contacts.config.json`, *not* `.METADATA.json`, so the
  second-brain resources sync never sweeps it into a lookup index.
- `README.md` — human-facing summary.

All mutations go through the **deterministic CLI**, never by hand:

```
python3 ~/.agents/skills/contacts/scripts/contacts.py <cmd>
```

Columns: `id, name, job, company, email, phone, preferred_comms,
address, tags, birthday, notes, created_date, last_updated`. `job`,
`company`, `email`, `phone`, `preferred_comms` (`email`/`whatsapp`/
`phone`), and `notes` mirror the common fields of an external people DB;
the rest are additions (`address`, `tags` semicolon-separated,
`birthday`, and the `id`/timestamp bookkeeping the script maintains
automatically).

## Commands

| Intent | Command |
|---|---|
| Add a contact | `contacts.py add --name "Patrick Doe" --job Accountant --email p@x.com --phone "+1 555 123 4567" --preferred-comms email --address "12 Main St" --tags "service-provider;finance" --notes "..."` |
| Edit a contact | `contacts.py edit <id-or-name> --phone "..."` (only passed fields change; `last_updated` bumped) |
| Delete a contact | `contacts.py delete <id-or-name>` |
| Search | `contacts.py find "<text>"` (all fields) or `--field job` to restrict |
| List | `contacts.py list [--tag family] [--job plumber] [--pretty]` |
| Show one | `contacts.py get <id-or-name>` |
| Fallback info | `contacts.py notion` |

`--name` is required on `add`. `edit`/`delete`/`get` accept either the
`id` (`C003`) or a name; an ambiguous name errors and lists candidates
so you can re-run with the id (this keeps mutations deterministic).
Data commands print JSON (add `--pretty` for a table); mutations print
the affected record as JSON. **Confirm before `delete`**, and before an
`edit` that overwrites an existing non-empty field.

## Two ways the user asks for a contact

Support both, using `find`:

- **By name** — "What is Patrick's address?", "Give me Maria's number"
  → `contacts.py find "patrick"`, then read the requested field.
- **By role / other field** — "Who is my accountant?", "What's my
  plumber's number?", "Who do we use for X?"
  → `contacts.py find accountant --field job` (or search all fields).
  The `job` column is the key for role lookups.

`find` matches case-insensitive substrings across name, job, company,
email, phone, address, tags, and notes, so queries anchored on any
field work.

## Lookup precedence — local CSV first, then fallback

When the user asks for a contact's name / email / phone / address (by
name **or** by role):

1. **Check memories first** — if a `[[...]]` memory already holds the
   answer, use it.
2. **Search the local CSV** — `contacts.py find …`. This is the source
   of truth and always wins.

<!-- brain:ext contacts:fallback -->

If the local CSV has no match and no fallback is configured, say so
plainly rather than guessing.

## Presenting lookup results — always a table

Whenever you answer a contact lookup (by name or by role), **present the
result as a clearly formatted markdown table** so it's easy to read and
easy to relocate later by scrolling the chat transcript. This applies to
every retrieval answer, not just mutations — **including trivial
single-field answers** ("what's her number?"). There is no lookup
small enough to skip the table; a bare prose answer is a defect.

Rules for the table:

- **Lead with one short sentence** naming who was found (e.g. "Your
  accountant is **Nam Nguyen** (Premier Tax Advisors)."), then the
  table. Keep any caveats *below* the table.
- **One column per contact, fields as rows** (transposed) when
  returning one or two contacts — it reads best and keeps long
  values like email/address on their own line. With three or more
  matches, use **one row per contact** with columns for the fields
  that matter to the question.
- **Include the fields the user needs plus the standard identifiers.**
  At minimum: Name, Job, Company, Email, Phone/WhatsApp, Preferred
  comms. Add Address / Notes / tags when relevant to the question or
  present in the record. Omit fields that are empty for every match
  rather than showing blank rows.
- **Render email, phone, and address as inline code** (backticks) so
  the user can select and copy them cleanly from the terminal.
- **Note the source** in a trailing line under the table when the
  match came from a fallback rather than the local CSV, so the user
  knows it isn't in their contacts book yet.

Transposed shape (one or two contacts):

```markdown
Your accountant is **Nam Nguyen** (Premier Tax Advisors).

|                | Nam Nguyen |
|----------------|------------|
| Job            | Accountant; CPA/Taxes |
| Company        | Premier Tax Advisors |
| Email          | `nam@premiertaxadvisors.com` |
| Phone/WhatsApp | `(408) 513-5537` |
| Preferred comms| Email |
```

Row-per-contact shape (three or more matches):

```markdown
Found 3 contacts matching "plumber":

| Name | Company | Phone/WhatsApp | Preferred comms |
|---|---|---|---|
| … | … | `…` | … |
```

This is a **display convention only** — it does not replace the
[additions table](../second-brain/SKILL.md#always-end-with-an-additions-table)
required after a mutation. A pure lookup (no add/edit/delete) ends with
the contact table above; a lookup that then saves a contact still ends
with the additions table per [After any contacts mutation](#after-any-contacts-mutation).

## After any contacts mutation

Adding / editing / deleting a contact is a brain change: end the
response with the
[additions table](../second-brain/SKILL.md#always-end-with-an-additions-table)
(path `<brain>/resources/contacts/contacts.csv`) and run the
[cleanup script](../second-brain/SKILL.md#end-of-session-clean-up-tool-byproducts):

```
bash ~/.agents/skills/second-brain/cleanup.sh
```

No `reindex.py` run is needed — the contacts CSV is a standalone book, not
a derived lookup index.
