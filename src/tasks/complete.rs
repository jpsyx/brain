//! `tasks complete <id>` — mark a task done in `~/brain/tasks/{tasks,habits}.csv`.
//!
//! Native Rust completion: set status/completed_date/last_touched, spawn the
//! next habit occurrence, and migrate chunked-task `mit` to the next chunk.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use chrono::{Datelike, Local, NaiveDate};

use crate::theme::Theme;

type Row = BTreeMap<String, String>;

#[derive(Debug, Clone)]
struct CsvFile {
    header: Vec<String>,
    rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Task,
    Habit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResult {
    pub kind: CompletionKind,
    pub task_id: String,
    pub task_name: String,
    pub next_id: Option<String>,
    pub next_due: Option<String>,
    pub mit_migrated_to: Option<String>,
    pub project: Option<String>,
    pub linear_issue: Option<String>,
}

/// Normalize a user-supplied ID into the canonical `T###` / `H###` form.
///
/// Accepts: `t123`, `T123`, `123` (assumed task), `h43`, `H43`. Any other
/// shape returns an error explaining the accepted forms.
pub fn normalize_id(raw: &str) -> Result<String> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("ID is required (try t123, T123, 123, or h43)");
    }
    let lower = s.to_ascii_lowercase();
    let (prefix, digits) = match lower.as_bytes().first() {
        Some(b't') => ('T', &lower[1..]),
        Some(b'h') => ('H', &lower[1..]),
        _ => ('T', lower.as_str()),
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("'{raw}' is not a valid ID (try t123, T123, 123, or h43)");
    }
    // Parse + reformat to drop any leading zeros (T0123 → T123) but keep the
    // exact value the user meant.
    let n: u32 = digits
        .parse()
        .map_err(|e| anyhow!("invalid number in ID '{raw}': {e}"))?;
    Ok(format!("{prefix}{n}"))
}

pub fn run(raw_id: &str) -> Result<()> {
    crate::logging::log(format!("tasks complete raw_id={raw_id}"));
    let root = crate::paths::brain_root()?;
    crate::logging::log(format!("complete root {}", root.display()));
    let today = Local::now().date_naive();
    let result = complete_in_root_with_today(&root, raw_id, today)?;
    crate::logging::log(format!(
        "complete result kind={:?} id={}",
        result.kind, result.task_id
    ));
    print_result(&result);
    Ok(())
}

pub fn complete_in_root(root: &Path, raw_id: &str) -> Result<CompletionResult> {
    complete_in_root_with_today(root, raw_id, Local::now().date_naive())
}

pub fn complete_in_root_with_today(
    root: &Path,
    raw_id: &str,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let tasks_dir = root.join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    if let Ok(normalized) = normalize_id(raw_id) {
        crate::logging::log(format!("complete normalized_id={normalized}"));
    }
    crate::logging::log(format!("read tasks csv {}", tasks_path.display()));
    let mut tasks = read_csv(&tasks_path)?;
    crate::logging::log(format!("read habits csv {}", habits_path.display()));
    let mut habits = read_csv(&habits_path)?;
    let located = locate(&tasks, &habits, raw_id)?;
    match located {
        Located::Task(idx) => {
            let result = complete_task(&mut tasks, idx, today)?;
            crate::logging::log(format!("write tasks csv {}", tasks_path.display()));
            write_csv(&tasks_path, &tasks)?;
            Ok(result)
        }
        Located::Habit(idx) => {
            let result = complete_habit(&tasks_dir, &mut habits, idx, today)?;
            crate::logging::log(format!("write habits csv {}", habits_path.display()));
            write_csv(&habits_path, &habits)?;
            Ok(result)
        }
    }
}

fn print_result(result: &CompletionResult) {
    let theme = Theme::active();
    match result.kind {
        CompletionKind::Task => {
            eprintln!(
                "{} {}  {}",
                theme.success("done:"),
                theme.accent(&result.task_id),
                theme.value(&result.task_name)
            );
            if let Some(id) = &result.mit_migrated_to {
                eprintln!("  {} {}", theme.info("MIT migrated to"), theme.accent(id));
            }
            if let Some(project) = &result.project {
                eprintln!(
                    "  {} {}; {}",
                    theme.warning("still linked to project"),
                    theme.value(project),
                    theme.muted("run /todo sync to refresh")
                );
            }
            if let Some(issue) = &result.linear_issue {
                eprintln!(
                    "  {} {} {}",
                    theme.warning("LINEAR:"),
                    theme.accent(issue),
                    theme.muted("close this issue too")
                );
            }
        }
        CompletionKind::Habit => {
            eprintln!(
                "{} {}  {}  {}",
                theme.success("done:"),
                theme.accent(&result.task_id),
                theme.value(&result.task_name),
                theme.muted("(habit)")
            );
            if let (Some(id), Some(due)) = (&result.next_id, &result.next_due) {
                eprintln!(
                    "  {} {} {} {}",
                    theme.info("next occurrence:"),
                    theme.accent(id),
                    theme.muted("due"),
                    theme.value(due)
                );
            }
        }
    }
}

fn complete_task(csv: &mut CsvFile, idx: usize, today: NaiveDate) -> Result<CompletionResult> {
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

fn complete_habit(
    tasks_dir: &Path,
    csv: &mut CsvFile,
    idx: usize,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let today_s = today.to_string();
    let (task_id, task_name, next_due, completed_row) = {
        let row = csv
            .rows
            .get_mut(idx)
            .ok_or_else(|| anyhow!("habit row disappeared"))?;
        row.insert("status".to_owned(), "done".to_owned());
        row.insert("completed_date".to_owned(), today_s.clone());
        touch_row(row, &today_s);
        (
            field(row, "task_id"),
            name(row),
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
    let mut next = completed_row;
    next.insert("task_id".to_owned(), next_id.clone());
    next.insert("status".to_owned(), "not_started".to_owned());
    next.insert("due_date".to_owned(), next_due.clone());
    next.insert("completed_date".to_owned(), String::new());
    next.insert("created_date".to_owned(), today_s.clone());
    next.insert("last_touched".to_owned(), today_s);
    csv.rows.push(next);
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

fn read_csv(path: &Path) -> Result<CsvFile> {
    if !path.exists() {
        return Ok(CsvFile {
            header: Vec::new(),
            rows: Vec::new(),
        });
    }
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_path(path)?;
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
    Ok(CsvFile { header, rows })
}

fn write_csv(path: &Path, csv: &CsvFile) -> Result<()> {
    let mut writer = csv::WriterBuilder::new().from_path(path)?;
    writer.write_record(&csv.header)?;
    for row in &csv.rows {
        let record: Vec<String> = csv
            .header
            .iter()
            .map(|column| row.get(column).cloned().unwrap_or_default())
            .collect();
        writer.write_record(record)?;
    }
    writer.flush()?;
    Ok(())
}

fn ensure_column(csv: &mut CsvFile, column: &str) {
    if !csv.header.iter().any(|existing| existing == column) {
        csv.header.push(column.to_owned());
    }
}

#[derive(Debug, Clone, Copy)]
enum Located {
    Task(usize),
    Habit(usize),
}

fn locate(tasks: &CsvFile, habits: &CsvFile, raw: &str) -> Result<Located> {
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

fn find_exact_id(rows: &[Row], id: &str) -> Option<usize> {
    rows.iter()
        .position(|row| row.get("task_id").is_some_and(|value| value.trim() == id))
}

fn find_fuzzy(tasks: &CsvFile, habits: &CsvFile, needle: &str) -> Result<Located> {
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

fn new_habit_id(tasks_dir: &Path, csv: &CsvFile) -> Result<String> {
    let path = tasks_dir.join(".habits_next_id");
    let next = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or_else(|| max_existing_id(&csv.rows, 'H') + 1);
    std::fs::write(path, format!("{}\n", next + 1))?;
    Ok(format!("H{next}"))
}

fn max_existing_id(rows: &[Row], prefix: char) -> u32 {
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

fn next_due(due: &str, interval: u32, unit: &str, today: NaiveDate) -> Result<String> {
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

fn add_interval(date: NaiveDate, interval: u32, unit: &str) -> Result<NaiveDate> {
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

fn add_months(date: NaiveDate, months: u32) -> Result<NaiveDate> {
    let zero_based_target = date.month0() + months;
    let year = date.year() + i32::try_from(zero_based_target / 12)?;
    let month = (zero_based_target % 12) + 1;
    let day = date.day().min(last_day_of_month(year, month)?);
    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| anyhow!("habit recurrence date overflowed"))
}

fn last_day_of_month(year: i32, month: u32) -> Result<u32> {
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

fn parse_date(value: &str) -> Option<NaiveDate> {
    let date = value.trim().split('T').next().unwrap_or_default();
    if date.is_empty() {
        None
    } else {
        NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
    }
}

fn migrate_mit_to_next_chunk(
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

fn parse_chunk_name(name: &str) -> Option<(String, u32, u32)> {
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

fn touch_row(row: &mut Row, today: &str) {
    row.insert("last_touched".to_owned(), today.to_owned());
}

fn field(row: &Row, column: &str) -> String {
    row.get(column).cloned().unwrap_or_default()
}

fn name(row: &Row) -> String {
    nonempty(row, "task_name").unwrap_or_else(|| "(unnamed)".to_owned())
}

fn nonempty(row: &Row, column: &str) -> Option<String> {
    let value = field(row, column);
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{CompletionKind, complete_in_root_with_today, normalize_id};
    use chrono::NaiveDate;

    #[test]
    fn bare_number_assumes_task_prefix() {
        assert_eq!(normalize_id("123").unwrap(), "T123");
    }

    #[test]
    fn lowercase_t_becomes_uppercase() {
        assert_eq!(normalize_id("t42").unwrap(), "T42");
    }

    #[test]
    fn lowercase_h_becomes_uppercase() {
        assert_eq!(normalize_id("h7").unwrap(), "H7");
    }

    #[test]
    fn leading_zeros_are_stripped() {
        assert_eq!(normalize_id("T00123").unwrap(), "T123");
        assert_eq!(normalize_id("h007").unwrap(), "H7");
    }

    #[test]
    fn empty_input_errors() {
        assert!(normalize_id("").is_err());
        assert!(normalize_id("   ").is_err());
    }

    #[test]
    fn non_digit_after_prefix_errors() {
        assert!(normalize_id("Tfoo").is_err());
        assert!(normalize_id("h-1").is_err());
    }

    #[test]
    fn completing_a_task_marks_done_and_touched_in_tasks_csv() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("tasks.csv"),
            "task_id,task_name,task_type,status,completed_date,last_touched,project,linear_issue\n\
             T1,Ship native complete,mit,not_started,,,alpha,LIN-1\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("habits.csv"),
            "task_id,task_name,status,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched\n",
        )
        .unwrap();

        let result = complete_in_root_with_today(
            dir.path(),
            "T1",
            NaiveDate::from_ymd_opt(2026, 7, 26).unwrap(),
        )
        .unwrap();

        assert_eq!(result.kind, CompletionKind::Task);
        let written = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
        assert!(
            written.contains("T1,Ship native complete,mit,done,2026-07-26,2026-07-26,alpha,LIN-1")
        );
    }

    #[test]
    fn completing_a_habit_spawns_the_next_occurrence() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("tasks.csv"),
            "task_id,task_name,status,completed_date,last_touched\n",
        )
        .unwrap();
        std::fs::write(tasks_dir.join(".habits_next_id"), "2\n").unwrap();
        std::fs::write(
            tasks_dir.join("habits.csv"),
            "task_id,task_name,status,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched\n\
             H1,Morning pages,not_started,2026-07-24,1,days,2026-07-24,,\n",
        )
        .unwrap();

        let result = complete_in_root_with_today(
            dir.path(),
            "H1",
            NaiveDate::from_ymd_opt(2026, 7, 26).unwrap(),
        )
        .unwrap();

        assert_eq!(result.kind, CompletionKind::Habit);
        assert_eq!(result.next_due.as_deref(), Some("2026-07-27"));
        let written = std::fs::read_to_string(tasks_dir.join("habits.csv")).unwrap();
        assert!(
            written.contains(
                "H1,Morning pages,done,2026-07-24,1,days,2026-07-24,2026-07-26,2026-07-26"
            )
        );
        assert!(written.contains("H2,Morning pages,not_started,2026-07-27,1,days,2026-07-26,,"));
        assert_eq!(
            std::fs::read_to_string(tasks_dir.join(".habits_next_id")).unwrap(),
            "3\n"
        );
    }
}
