#!/usr/bin/env python3
"""Deterministic CRUD + search for the brain's local contacts book.

The contacts book is a single CSV at
  ~/brain/resources/contacts/contacts.csv
with a stable `id` per contact (C001, C002, ...). This script is the
*only* correct way to mutate that CSV — it handles id assignment,
timestamps, quoting, and field validation so the data stays clean.

Columns:
  id, name, job, company, email, phone, preferred_comms, address,
  tags, birthday, notes, created_date, last_updated

Lookup precedence used by the /contacts skill: search this local CSV
first (`find`); only if nothing matches, and if a fallback is configured,
does the skill fall back to it (see `notion` subcommand / contacts.config.json).

Usage:
  contacts.py add --name "Patrick Doe" --job Accountant --email p@x.com \
      --phone "+1 555 123 4567" --preferred-comms email \
      --address "12 Main St, NYC" --tags "service-provider;finance" \
      --notes "Handles taxes"
  contacts.py edit C003 --phone "+1 555 000 0000"     # id or name
  contacts.py delete C003                             # id or name
  contacts.py list                                    # all, JSON
  contacts.py list --tag family --pretty              # filtered table
  contacts.py find "patrick"                          # search all fields
  contacts.py find accountant --field job             # search one field
  contacts.py get C003                                # one contact by id
  contacts.py notion                                  # print Notion fallback

Data commands (list/find/get) print JSON to stdout (or a table with
--pretty). Mutations (add/edit/delete) print the affected record as JSON.
Errors go to stderr with a non-zero exit code.
"""
import argparse
import csv
import json
import re
import sys
from datetime import date
from pathlib import Path

BRAIN = Path.home() / "brain"
CONTACTS_DIR = BRAIN / "resources" / "contacts"
CSV_PATH = CONTACTS_DIR / "contacts.csv"
CONFIG_PATH = CONTACTS_DIR / "contacts.config.json"

COLUMNS = [
    "id", "name", "job", "company", "email", "phone", "preferred_comms",
    "address", "tags", "birthday", "notes", "created_date", "last_updated",
]

# Fields searched by `find` when --field is not given.
SEARCH_FIELDS = [
    "name", "job", "company", "email", "phone", "address", "tags", "notes",
]

PREFERRED_COMMS = {"email", "whatsapp", "phone"}


def die(msg: str, code: int = 1) -> "NoReturn":  # type: ignore[name-defined]
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(code)


def today() -> str:
    return date.today().isoformat()


def read_contacts() -> list[dict]:
    if not CSV_PATH.exists():
        return []
    with CSV_PATH.open(newline="", encoding="utf-8") as f:
        return [dict(row) for row in csv.DictReader(f)]


def write_contacts(rows: list[dict]) -> None:
    CONTACTS_DIR.mkdir(parents=True, exist_ok=True)
    # Stable order: by numeric id.
    rows = sorted(rows, key=lambda r: _id_num(r.get("id", "")))
    with CSV_PATH.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=COLUMNS, extrasaction="ignore")
        w.writeheader()
        for r in rows:
            w.writerow({c: r.get(c, "") for c in COLUMNS})


def _id_num(cid: str) -> int:
    m = re.match(r"[Cc](\d+)$", cid or "")
    return int(m.group(1)) if m else 0


def next_id(rows: list[dict]) -> str:
    n = max((_id_num(r.get("id", "")) for r in rows), default=0) + 1
    return f"C{n:03d}"


def norm_comms(value: str) -> str:
    v = (value or "").strip().lower()
    if v and v not in PREFERRED_COMMS:
        die(f"--preferred-comms must be one of {sorted(PREFERRED_COMMS)}, got {value!r}")
    return v


def resolve(rows: list[dict], ident: str) -> dict:
    """Resolve an id (C###) or a name to exactly one contact, else error."""
    ident = ident.strip()
    by_id = [r for r in rows if r["id"].lower() == ident.lower()]
    if by_id:
        return by_id[0]
    exact = [r for r in rows if r["name"].lower() == ident.lower()]
    if len(exact) == 1:
        return exact[0]
    if len(exact) > 1:
        _die_ambiguous(exact, ident)
    sub = [r for r in rows if ident.lower() in r["name"].lower()]
    if len(sub) == 1:
        return sub[0]
    if len(sub) > 1:
        _die_ambiguous(sub, ident)
    die(f"no contact matches {ident!r} (by id or name)")


def _die_ambiguous(matches: list[dict], ident: str) -> "NoReturn":  # type: ignore[name-defined]
    lines = "\n".join(f"  {r['id']}  {r['name']}"
                      + (f"  ({r['job']})" if r.get("job") else "")
                      for r in matches)
    die(f"{ident!r} matches multiple contacts; specify the id:\n{lines}")


# ---- field flags shared by add/edit -------------------------------------

FIELD_FLAGS = [
    ("name", "--name", "Full name (title)"),
    ("job", "--job", 'Role / job, e.g. "Accountant", "Plumber"'),
    ("company", "--company", "Company or organization"),
    ("email", "--email", "Email address"),
    ("phone", "--phone", "Phone / WhatsApp number"),
    ("preferred_comms", "--preferred-comms", "email | whatsapp | phone"),
    ("address", "--address", "Postal / physical address"),
    ("tags", "--tags", 'Semicolon-separated tags, e.g. "family;medical"'),
    ("birthday", "--birthday", "Birthday, YYYY-MM-DD"),
    ("notes", "--notes", "Freeform notes"),
]


def add_field_args(p: argparse.ArgumentParser) -> None:
    for field, flag, help_ in FIELD_FLAGS:
        p.add_argument(flag, dest=field, help=help_)


def collect_fields(args) -> dict:
    out = {}
    for field, _flag, _h in FIELD_FLAGS:
        val = getattr(args, field, None)
        if val is not None:
            out[field] = norm_comms(val) if field == "preferred_comms" else val
    return out


# ---- subcommands ---------------------------------------------------------

def cmd_add(args) -> None:
    rows = read_contacts()
    fields = collect_fields(args)
    if not fields.get("name"):
        die("--name is required to add a contact")
    rec = {c: "" for c in COLUMNS}
    rec.update(fields)
    rec["id"] = next_id(rows)
    rec["created_date"] = today()
    rec["last_updated"] = today()
    rows.append(rec)
    write_contacts(rows)
    _emit_mutation("added", rec, args)


def cmd_edit(args) -> None:
    rows = read_contacts()
    rec = resolve(rows, args.ident)
    fields = collect_fields(args)
    if not fields:
        die("no fields given to edit (pass at least one --field)")
    rec.update(fields)
    rec["last_updated"] = today()
    write_contacts(rows)
    _emit_mutation("edited", rec, args)


def cmd_delete(args) -> None:
    rows = read_contacts()
    rec = resolve(rows, args.ident)
    rows = [r for r in rows if r["id"] != rec["id"]]
    write_contacts(rows)
    _emit_mutation("deleted", rec, args)


def cmd_list(args) -> None:
    rows = read_contacts()
    if args.tag:
        t = args.tag.lower()
        rows = [r for r in rows
                if t in [x.strip().lower() for x in (r.get("tags") or "").split(";")]]
    if args.job:
        rows = [r for r in rows if args.job.lower() in (r.get("job") or "").lower()]
    _emit_records(rows, args)


def cmd_find(args) -> None:
    rows = read_contacts()
    q = args.query.lower()
    fields = [args.field] if args.field else SEARCH_FIELDS
    hits = [r for r in rows
            if any(q in (r.get(f) or "").lower() for f in fields)]
    _emit_records(hits, args)


def cmd_get(args) -> None:
    rows = read_contacts()
    rec = resolve(rows, args.ident)
    _emit_records([rec], args)


def cmd_notion(args) -> None:
    cfg = json.loads(CONFIG_PATH.read_text()) if CONFIG_PATH.exists() else {}
    fb = cfg.get("notion_fallback", {})
    if not fb:
        die("no Notion fallback configured in contacts.config.json")
    print(json.dumps(fb, indent=2, ensure_ascii=False))


# ---- output --------------------------------------------------------------

def _emit_records(rows: list[dict], args) -> None:
    if getattr(args, "pretty", False):
        _print_table(rows)
    else:
        print(json.dumps(rows, indent=2, ensure_ascii=False))


def _emit_mutation(action: str, rec: dict, args) -> None:
    print(json.dumps({"action": action, "contact": rec},
                     indent=2, ensure_ascii=False))


def _print_table(rows: list[dict]) -> None:
    if not rows:
        print("(no matching contacts)")
        return
    cols = ["id", "name", "job", "company", "phone", "email"]
    widths = {c: max(len(c), *(len(str(r.get(c, ""))) for r in rows)) for c in cols}
    header = "  ".join(c.ljust(widths[c]) for c in cols)
    print(header)
    print("  ".join("-" * widths[c] for c in cols))
    for r in rows:
        print("  ".join(str(r.get(c, "")).ljust(widths[c]) for c in cols))


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)

    a = sub.add_parser("add", help="add a new contact")
    add_field_args(a)
    a.set_defaults(func=cmd_add)

    e = sub.add_parser("edit", help="edit a contact by id or name")
    e.add_argument("ident", help="contact id (C###) or name")
    add_field_args(e)
    e.set_defaults(func=cmd_edit)

    d = sub.add_parser("delete", help="delete a contact by id or name")
    d.add_argument("ident", help="contact id (C###) or name")
    d.set_defaults(func=cmd_delete)

    ls = sub.add_parser("list", help="list all contacts")
    ls.add_argument("--tag", help="only contacts carrying this tag")
    ls.add_argument("--job", help="only contacts whose job contains this")
    ls.add_argument("--pretty", action="store_true", help="table instead of JSON")
    ls.set_defaults(func=cmd_list)

    f = sub.add_parser("find", help="search contacts")
    f.add_argument("query", help="text to search for")
    f.add_argument("--field", choices=SEARCH_FIELDS,
                   help="restrict search to one field (default: all)")
    f.add_argument("--pretty", action="store_true", help="table instead of JSON")
    f.set_defaults(func=cmd_find)

    g = sub.add_parser("get", help="show one contact by id or name")
    g.add_argument("ident", help="contact id (C###) or name")
    g.add_argument("--pretty", action="store_true", help="table instead of JSON")
    g.set_defaults(func=cmd_get)

    n = sub.add_parser("notion", help="print the Notion fallback DB info")
    n.set_defaults(func=cmd_notion)

    return p


def main(argv=None) -> None:
    args = build_parser().parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main()
