use crate::tasks::complete::{CsvFile, Row};

pub(super) fn row(id: &str, name: &str) -> Row {
    Row::from([
        ("task_id".into(), id.into()),
        ("task_name".into(), name.into()),
    ])
}

pub(super) fn csv(rows: Vec<Row>) -> CsvFile {
    CsvFile {
        header: vec!["task_id".into(), "task_name".into()],
        rows,
    }
}

pub(super) fn needles(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
