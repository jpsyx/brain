
fn rows(root: &Path, relative: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut reader = csv::Reader::from_path(root.join(relative)).unwrap();
    let headers = reader.headers().unwrap().clone();
    reader
        .records()
        .map(|record| {
            let record = record.unwrap();
            let row = headers
                .iter()
                .zip(record.iter())
                .map(|(column, value)| (column.to_owned(), value.to_owned()))
                .collect::<BTreeMap<_, _>>();
            (row["task_id"].clone(), row)
        })
        .collect()
}
