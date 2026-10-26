---
name: contacts
description: Use when the user asks about a person or service provider in their contacts book - "what's Maria's number?", "who is my accountant?", "what's my plumber's email?", "add a contact", "update so-and-so's address" - or any read/write to the local contacts CSV at `resources/contacts/`. Handles add / edit / delete / search / list via a deterministic CLI.
---

# contacts

The selected workspace's contact book lives at `<brain>/resources/contacts/`.
Use `brain contacts find`, `get`, or `list` for every lookup. Memory may help
choose the query but never replaces canonical lookup. Do not hand-read the CSV
or guess against a fallback service; discover any configured fallback through
`brain contacts fallback` and the hook below.

## Storage

The contacts book lives at `<brain>/resources/contacts/`:

- `contacts.csv` - one row per contact, keyed by a stable `id`
  (`C001`, `C002`, …).
- `contacts.config.json` - column list + an optional fallback pointer
  (see [Lookup precedence](#lookup-precedence--local-csv-first-then-fallback)).
  Named `contacts.config.json`, *not* `.METADATA.json`, so the
  resource reindex never sweeps it into a lookup index.
- `README.md` - human-facing summary.

All mutations go through the **deterministic CLI**, never by hand:

```
brain contacts <cmd>
```

Columns: `id, name, job, company, email, phone, preferred_comms,
address, tags, birthday, notes, created_date, last_updated`. `job`,
`company`, `email`, `phone`, `preferred_comms` (`email`/`whatsapp`/
`phone`), and `notes` mirror the common fields of an external people DB;
the rest are additions (`address`, `tags` semicolon-separated,
`birthday`, and the `id`/timestamp bookkeeping the CLI maintains
automatically).

## Commands

| Intent | Command |
|---|---|
| Add a contact | `brain contacts add --name "Patrick Doe" --job Accountant --email p@x.com --phone "+1 555 123 4567" --preferred-comms email --address "12 Main St" --tags "service-provider;finance" --notes "..."` |
| Edit a contact | `brain contacts edit <id-or-name> --phone "..."` (only passed fields change; `last_updated` bumped) |
| Delete a contact | `brain contacts delete <id-or-name>` |
| Search | `brain contacts find "<text>"` (all fields) or `--field job` to restrict |
| List | `brain contacts list [--tag family] [--job plumber] [--pretty]` |
| Show one | `brain contacts get <id-or-name>` |
| Fallback info | `brain contacts fallback` |

`--name` is required on `add`. `edit`/`delete`/`get` accept either the
`id` (`C003`) or a name; an ambiguous name errors and lists candidates
so you can re-run with the id (this keeps mutations deterministic).
Data commands print JSON (add `--pretty` for a table); mutations print
the affected record as JSON. **Confirm before `delete`**, and before an
`edit` that overwrites an existing non-empty field.

## Two ways the user asks for a contact

Support both, using `find`:

- **By name** - "What is Patrick's address?", "Give me Maria's number"
  → `brain contacts find "patrick"`, then read the requested field.
- **By role / other field** - "Who is my accountant?", "What's my
  plumber's number?", "Who do we use for X?"
  → `brain contacts find accountant --field job` (or search all fields).
  The `job` column is the key for role lookups.

`find` matches case-insensitive substrings across name, job, company,
email, phone, address, tags, and notes, so queries anchored on any
field work.

<a id="lookup-precedence--local-csv-first-then-fallback"></a>

## Lookup precedence

When the user asks for a contact's name / email / phone / address (by
name **or** by role):

1. Use remembered context only to choose an identifying name or role query.
2. Search with `brain contacts find`; the local record is authoritative.
3. If no local match exists, inspect `brain contacts fallback` before using
   any configured fallback through its documented integration.

<!-- brain:ext contacts:fallback -->

If the local CSV has no match and no fallback is configured, say so
plainly rather than guessing.

<a id="presenting-lookup-results--always-a-table"></a>

## Presenting lookup results

Return the requested facts first and identify the matching person. Include
relevant available identifiers: Name, Job, Company, Email, Phone/WhatsApp, and
Preferred comms; add Address, Notes, or tags when useful. Omit fields empty for
every match. For multiple matches, distinguish candidates and resolve ambiguity
before mutation. Preserve exact contact values and disclose when a result came
from a fallback rather than the local book. Never fabricate a missing field.
These are content requirements; the workspace response policy determines
presentation and how much supporting detail fits the active channel.

## After any contacts mutation

Report the changed record, fields, and actual outcome. No `brain reindex`
run is needed: the contacts CSV is a standalone book, not a derived lookup.
