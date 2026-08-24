pub(super) fn complete_task(
    csv: &mut CsvFile,
    idx: usize,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let today = today.to_string();
    let row = csv
        .rows
        .get_mut(idx)
        .ok_or_else(|| anyhow!("task row disappeared"))?;
    row.insert("status".to_owned(), "done".to_owned());
    row.insert("completed_date".to_owned(), today.clone());
    touch_row(row, &today);
    let task_id = field(row, "task_id");
    let task_name = name(row);
    let project = nonempty(row, "project");
    let linear_issue = nonempty(row, "linear_issue");
    let mit_migrated_to = migrate_mit_to_next_chunk(&mut csv.rows, idx, &today);
    ensure_column(csv, "last_touched");
    Ok(CompletionResult {
        kind: CompletionKind::Task,
        task_id,
        task_name,
        next_id: None,
        next_due: None,
        mit_migrated_to,
        project,
        linear_issue,
    })
}

pub(super) fn complete_habit(
    tasks_dir: &Path,
    csv: &mut CsvFile,
    idx: usize,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let today_s = today.to_string();
    let (task_id, task_name) = {
        let row = csv
            .rows
            .get_mut(idx)
            .ok_or_else(|| anyhow!("habit row disappeared"))?;
        row.insert("status".to_owned(), "done".to_owned());
        row.insert("completed_date".to_owned(), today_s.clone());
        touch_row(row, &today_s);
        (field(row, "task_id"), name(row))
    };
    let (next_id, next_due) = spawn_next_occurrence(tasks_dir, csv, idx, today)?;
    ensure_column(csv, "last_touched");
    Ok(CompletionResult {
        kind: CompletionKind::Habit,
        task_id,
        task_name,
        next_id: Some(next_id),
        next_due: Some(next_due),
        mit_migrated_to: None,
        project: None,
        linear_issue: None,
    })
}

/// Append the next occurrence of the habit at `source_idx`, anchoring its
/// recurrence to that row's `due_date`/`recur_*` and dating it to the first
/// occurrence strictly after `today`. Returns `(next_id, next_due)`.
///
/// The new row is a clone of the source with a fresh id, `status=not_started`,
/// the recurred `due_date`, an empty `completed_date`, and today's
/// `created_date`/`last_touched`; every other column (name, priority, notes,
/// `ideal_time`, recurrence, …) carries over verbatim. Shared by completion
/// (spawn after marking today's instance done) and revive (respawn a lapsed
/// chain whose source is already `done`).
pub(crate) fn spawn_next_occurrence(
    tasks_dir: &Path,
    csv: &mut CsvFile,
    source_idx: usize,
    today: NaiveDate,
) -> Result<(String, String)> {
    let today_s = today.to_string();
    let (next_due, mut next) = {
        let row = csv
            .rows
            .get(source_idx)
            .ok_or_else(|| anyhow!("habit row disappeared"))?;
        (
            next_due(
                &field(row, "due_date"),
                field(row, "recur_interval").parse::<u32>().unwrap_or(1),
                &field(row, "recur_unit"),
                today,
            )?,
            row.clone(),
        )
    };
    let next_id = new_habit_id(tasks_dir, csv)?;
    next.insert("task_uuid".to_owned(), TaskUuid::new().to_string());
    next.insert("task_id".to_owned(), next_id.clone());
    next.insert("status".to_owned(), "not_started".to_owned());
    next.insert("due_date".to_owned(), next_due.clone());
    next.insert("completed_date".to_owned(), String::new());
    next.insert("created_date".to_owned(), today_s.clone());
    next.insert("last_touched".to_owned(), today_s);
    csv.rows.push(next);
    ensure_column(csv, "task_uuid");
    Ok((next_id, next_due))
}

pub(crate) fn read_csv(path: &Path) -> Result<CsvFile> {
    if !path.exists() {
        return Ok(CsvFile {
            header: Vec::new(),
            rows: Vec::new(),
        });
    }
    let bytes = std::fs::read(path)?;
    parse_csv_bytes(&bytes)
}

pub(crate) fn parse_csv_bytes(bytes: &[u8]) -> Result<CsvFile> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let header: Vec<String> = reader.headers()?.iter().map(str::to_owned).collect();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let mut row = Row::new();
        for (idx, column) in header.iter().enumerate() {
            row.insert(
                column.clone(),
                record.get(idx).unwrap_or_default().to_owned(),
            );
        }
        rows.push(row);
    }
    Ok(normalize_assignment_header(CsvFile { header, rows }))
}

pub(super) fn normalize_assignment_header(mut csv: CsvFile) -> CsvFile {
    let legacy_position = csv.header.iter().position(|column| column == "assignee");
    let has_canonical = csv.header.iter().any(|column| column == "assigned_to");
    let Some(legacy_position) = legacy_position else {
        return csv;
    };
    if has_canonical {
        csv.header.remove(legacy_position);
        for row in &mut csv.rows {
            row.remove("assignee");
        }
    } else {
        "assigned_to".clone_into(&mut csv.header[legacy_position]);
        for row in &mut csv.rows {
            let assignment = row.remove("assignee").unwrap_or_default();
            row.insert("assigned_to".to_owned(), assignment);
        }
    }
    csv
}

pub(crate) fn write_csv(path: &Path, csv: &CsvFile) -> Result<()> {
    std::fs::write(path, serialize_csv(csv)?)?;
    Ok(())
}

pub(crate) fn serialize_csv(csv: &CsvFile) -> Result<Vec<u8>> {
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record(&csv.header)?;
    for row in &csv.rows {
        let record: Vec<String> = csv
            .header
            .iter()
            .map(|column| row.get(column).cloned().unwrap_or_default())
            .collect();
        writer.write_record(record)?;
    }
    writer
        .into_inner()
        .map_err(csv::IntoInnerError::into_error)
        .map_err(Into::into)
}

pub(super) fn ensure_column(csv: &mut CsvFile, column: &str) {
    if !csv.header.iter().any(|existing| existing == column) {
        csv.header.push(column.to_owned());
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Located {
    Task(usize),
    Habit(usize),
}

pub(crate) fn locate(tasks: &CsvFile, habits: &CsvFile, raw: &str) -> Result<Located> {
    let needle = raw.trim();
    if needle.is_empty() {
        bail!("ID is required (try t123, T123, 123, or h43)");
    }
    if needle.chars().all(|c| c.is_ascii_digit()) {
        let n: u32 = needle
            .parse()
            .map_err(|e| anyhow!("invalid number in ID '{raw}': {e}"))?;
        let task_hit = find_exact_id(&tasks.rows, &format!("T{n}"));
        let habit_hit = find_exact_id(&habits.rows, &format!("H{n}"));
        return match (task_hit, habit_hit) {
            (Some(_), Some(_)) => {
                bail!("ambiguous: bare ID '{n}' matches both T{n} and H{n}; use the prefix")
            }
            (Some(idx), None) => Ok(Located::Task(idx)),
            (None, Some(idx)) => Ok(Located::Habit(idx)),
            (None, None) => bail!("no task matched '{raw}'"),
        };
    }
    if let Ok(id) = normalize_id(needle) {
        return if id.starts_with('H') {
            find_exact_id(&habits.rows, &id)
                .map(Located::Habit)
                .ok_or_else(|| anyhow!("no task matched '{raw}'"))
        } else {
            find_exact_id(&tasks.rows, &id)
                .map(Located::Task)
                .ok_or_else(|| anyhow!("no task matched '{raw}'"))
        };
    }
    find_fuzzy(tasks, habits, needle)
}

pub(super) fn find_exact_id(rows: &[Row], id: &str) -> Option<usize> {
    rows.iter()
        .position(|row| row.get("task_id").is_some_and(|value| value.trim() == id))
}

pub(super) fn find_fuzzy(tasks: &CsvFile, habits: &CsvFile, needle: &str) -> Result<Located> {
    let low = needle.to_ascii_lowercase();
    let mut hits = Vec::new();
    hits.extend(
        tasks
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| field(row, "task_name").to_ascii_lowercase().contains(&low))
            .map(|(idx, _)| Located::Task(idx)),
    );
    hits.extend(
        habits
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| field(row, "task_name").to_ascii_lowercase().contains(&low))
            .map(|(idx, _)| Located::Habit(idx)),
    );
    match hits.as_slice() {
        [hit] => Ok(*hit),
        [] => bail!("no task matched '{needle}'"),
        _ => bail!("ambiguous: {} tasks match '{needle}'", hits.len()),
    }
}

pub(super) fn new_habit_id(tasks_dir: &Path, csv: &CsvFile) -> Result<String> {
    let path = tasks_dir.join(".habits_next_id");
    let next = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or_else(|| max_existing_id(&csv.rows, 'H') + 1);
    std::fs::write(path, format!("{}\n", next + 1))?;
    Ok(format!("H{next}"))
}

pub(super) fn max_existing_id(rows: &[Row], prefix: char) -> u32 {
    rows.iter()
        .filter_map(|row| {
            field(row, "task_id")
                .strip_prefix(prefix)?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0)
}

pub(super) fn next_due(due: &str, interval: u32, unit: &str, today: NaiveDate) -> Result<String> {
    let mut date = parse_date(due)
        .ok_or_else(|| anyhow!("habit due_date is required to spawn the next occurrence"))?;
    let interval = interval.max(1);
    for _ in 0..600 {
        date = add_interval(date, interval, unit)?;
        if date > today {
            return Ok(date.to_string());
        }
    }
    bail!("could not fast-forward habit recurrence past today")
}

pub(super) fn add_interval(date: NaiveDate, interval: u32, unit: &str) -> Result<NaiveDate> {
    match unit {
        "days" | "" => date
            .checked_add_days(chrono::Days::new(u64::from(interval)))
            .ok_or_else(|| anyhow!("habit recurrence date overflowed")),
        "weeks" => date
            .checked_add_days(chrono::Days::new(u64::from(interval) * 7))
            .ok_or_else(|| anyhow!("habit recurrence date overflowed")),
        "months" => add_months(date, interval),
        other => bail!("unknown recur_unit: {other}"),
    }
}

pub(super) fn add_months(date: NaiveDate, months: u32) -> Result<NaiveDate> {
    let zero_based_target = date.month0() + months;
    let year = date.year() + i32::try_from(zero_based_target / 12)?;
    let month = (zero_based_target % 12) + 1;
    let day = date.day().min(last_day_of_month(year, month)?);
    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| anyhow!("habit recurrence date overflowed"))
}

pub(super) fn last_day_of_month(year: i32, month: u32) -> Result<u32> {
    let first_next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or_else(|| anyhow!("habit recurrence date overflowed"))?;
    Ok(first_next
        .pred_opt()
        .ok_or_else(|| anyhow!("habit recurrence date overflowed"))?
        .day())
}

pub(super) fn parse_date(value: &str) -> Option<NaiveDate> {
    let date = value.trim().split('T').next().unwrap_or_default();
    if date.is_empty() {
        None
    } else {
        NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
    }
}

pub(super) fn migrate_mit_to_next_chunk(
    rows: &mut [Row],
    completed_idx: usize,
    today: &str,
) -> Option<String> {
    let row = rows.get(completed_idx)?;
    let types = field(row, "task_type");
    if !types.split('|').any(|part| part == "mit") {
        return None;
    }
    let (base, idx, total) = parse_chunk_name(&field(row, "task_name"))?;
    if idx >= total {
        return None;
    }
    let target = format!("{base} ({}/{total})", idx + 1);
    let next = rows
        .iter_mut()
        .find(|candidate| field(candidate, "task_name").trim() == target)?;
    if field(next, "status").trim() == "done" {
        return None;
    }
    let next_types = field(next, "task_type");
    let mut parts: Vec<&str> = next_types
        .split('|')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.contains(&"mit") {
        return None;
    }
    parts.push("mit");
    next.insert("task_type".to_owned(), parts.join("|"));
    touch_row(next, today);
    Some(format!(
        "{}  {}",
        field(next, "task_id"),
        field(next, "task_name")
    ))
}

pub(crate) fn parse_chunk_name(name: &str) -> Option<(String, u32, u32)> {
    let trimmed = name.trim();
    let (base, suffix) = trimmed.rsplit_once(" (")?;
    let fraction = suffix.strip_suffix(')')?;
    let (idx, total) = fraction.split_once('/')?;
    Some((
        base.to_owned(),
        idx.parse::<u32>().ok()?,
        total.parse::<u32>().ok()?,
    ))
}

pub(super) fn touch_row(row: &mut Row, today: &str) {
    row.insert("last_touched".to_owned(), today.to_owned());
}

use super::{
    CompletionKind, CompletionResult, CsvFile, Datelike, NaiveDate, Path, Result, Row, TaskUuid,
    anyhow, bail, field, name, nonempty, normalize_id,
};
