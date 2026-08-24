mod defer_and_touch;
mod derived_sections;
mod document;
mod done;
mod shell;

use chrono::NaiveDate;

use crate::tasks::complete::Row;

/// Build a CSV row from `(column, value)` pairs.
pub(super) fn row(cells: &[(&str, &str)]) -> Row {
    cells
        .iter()
        .map(|(column, value)| ((*column).to_owned(), (*value).to_owned()))
        .collect()
}

pub(super) fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
}

/// A realistic agenda carrying every section the sync knows about plus a
/// section it doesn't, so preservation is checked against real content.
pub(super) const FULL_AGENDA: &str = "\
# Monday 2026-08-24

**Load:** 4 tasks, 3 habits
**Bottom line:** ship the sync.

## ❗ Most important

- [ ] ❗ **T535** Fix the sync (45m)
- [ ] ❗ **T536** Write the docs (30m)

## Suggested order

1. [ ] 09:00 | **T535** Fix the sync (45m)
2. [ ] 10:00 | **T536** Write the docs (30m)

## Cut order

1. **T536** Write the docs
2. **T535** Fix the sync

## Notes to self

Core has never heard of this section.
";
