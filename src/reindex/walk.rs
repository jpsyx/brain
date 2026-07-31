//! Filesystem walk for the projects/resources reindex.
//!
//! Thin IO shell: it enumerates `.METADATA.json` files, reads the colocated
//! `notes.md`, delegates the row mapping to the pure builders, sorts
//! deterministically, and writes the lookup CSV. The mapping/scanning logic
//! lives in the pure sibling modules and is tested there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::csvfmt::render_csv;
use super::notes::{NotesFlags, scan_notes};
use super::projects::{PROJECTS_HEADER, parse_project_meta, project_row};
use super::resources::{RESOURCES_HEADER, parse_resource_meta, resource_row};

const METADATA: &str = ".METADATA.json";

/// Outcome of rebuilding one lookup CSV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub path: PathBuf,
    pub rows: usize,
    pub skipped: usize,
    pub wrote: bool,
}

/// Rebuild `projects/projects-lookup.csv` from every direct
/// `projects/<name>/.METADATA.json`. Archived projects (under `archive/`) are
/// intentionally excluded.
pub fn rebuild_projects(root: &Path) -> io::Result<Report> {
    let dir = root.join("projects");
    let out = dir.join("projects-lookup.csv");
    if !dir.is_dir() {
        return Ok(Report {
            path: out,
            rows: 0,
            skipped: 0,
            wrote: false,
        });
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut skipped = 0;
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(text) = fs::read_to_string(path.join(METADATA)) else {
            continue;
        };
        let rel = relative(root, &path);
        match parse_project_meta(&text) {
            Some(meta) => rows.push(project_row(&meta, &rel)),
            None => skipped += 1,
        }
    }
    rows.sort_by(|a, b| a[0].cmp(&b[0])); // by name
    fs::write(&out, render_csv(&PROJECTS_HEADER, &rows))?;
    Ok(Report {
        path: out,
        rows: rows.len(),
        skipped,
        wrote: true,
    })
}

/// Rebuild `resources/zotero-lookup.csv` from every
/// `resources/**/.METADATA.json` plus its colocated `notes.md`.
pub fn rebuild_resources(root: &Path) -> io::Result<Report> {
    let dir = root.join("resources");
    let out = dir.join("zotero-lookup.csv");
    if !dir.is_dir() {
        return Ok(Report {
            path: out,
            rows: 0,
            skipped: 0,
            wrote: false,
        });
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut skipped = 0;
    for entry in WalkDir::new(&dir).into_iter().filter_map(Result::ok) {
        if entry.file_name() != METADATA {
            continue;
        }
        let meta_path = entry.path();
        let subject = meta_path.parent().unwrap_or(&dir);
        let Ok(text) = fs::read_to_string(meta_path) else {
            continue;
        };
        let flags = fs::read_to_string(subject.join("notes.md"))
            .map_or_else(|_| NotesFlags::empty(), |t| scan_notes(&t));
        let rel = relative(root, subject);
        match parse_resource_meta(&text) {
            Some(meta) => rows.push(resource_row(&meta, &rel, &flags)),
            None => skipped += 1,
        }
    }
    rows.sort_by(|a, b| a[6].cmp(&b[6])); // by directory
    fs::write(&out, render_csv(&RESOURCES_HEADER, &rows))?;
    Ok(Report {
        path: out,
        rows: rows.len(),
        skipped,
        wrote: true,
    })
}

/// Brain-root-relative, forward-slash path string.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, body).expect("write");
    }

    #[test]
    fn rebuild_projects_writes_sorted_rows_from_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            &root.join("projects/zeta/.METADATA.json"),
            r#"{"name":"zeta","namespace":"n","title":"Zeta","status":"in-progress","priority":"p2","due":"2026-01-01"}"#,
        );
        write(
            &root.join("projects/alpha/.METADATA.json"),
            r#"{"name":"alpha","namespace":"n","title":"Alpha","status":"blocked","priority":"p1"}"#,
        );

        let report = rebuild_projects(root).expect("rebuild");
        assert_eq!(report.rows, 2);
        let csv = fs::read_to_string(&report.path).expect("read csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "name,namespace,title,status,priority,due,directory");
        // sorted by name: alpha before zeta; missing due -> none
        assert_eq!(lines[1], "alpha,n,Alpha,blocked,p1,none,projects/alpha");
        assert_eq!(lines[2], "zeta,n,Zeta,in-progress,p2,2026-01-01,projects/zeta");
    }

    #[test]
    fn rebuild_resources_scans_notes_and_reflects_filesystem_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            &root.join("resources/adhd/paper/.METADATA.json"),
            r#"{"zotero_key":"K1","title":"A paper","authors":["Doe, J."],"year":"2020","item_type":"journalArticle","collection":"Read","tags":["read"],"attachments":[{"type":"pdf"}]}"#,
        );
        write(
            &root.join("resources/adhd/paper/notes.md"),
            "## Summary\n\nReal summary content.\n\n## Notes\n\n*No standalone user notes attached.*\n\n## Annotations\n\n> one\n\n> two\n",
        );

        let report = rebuild_resources(root).expect("rebuild");
        assert_eq!(report.rows, 1);
        let csv = fs::read_to_string(&report.path).expect("read csv");
        let row = csv.lines().nth(1).expect("data row");
        assert_eq!(
            row,
            "K1,A paper,\"Doe, J.\",2020,journalArticle,Read,resources/adhd/paper,yes,no,yes,no,2,read"
        );
    }

    #[test]
    fn missing_directories_do_not_error_and_do_not_write() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let report = rebuild_projects(tmp.path()).expect("rebuild");
        assert!(!report.wrote);
        assert_eq!(report.rows, 0);
    }
}
