//! Pure CSV rendering shared by the projects/resources reindex.
//!
//! Uses the `csv` crate so quoting matches the rest of brain's CSV handling
//! (fields containing a comma or quote are quoted; a `;`-joined field with no
//! comma is left bare), and the line terminator is a plain `\n`.

use csv::WriterBuilder;

/// Render a header row plus data rows into a CSV string.
#[must_use]
pub fn render_csv(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut wtr = WriterBuilder::new().from_writer(Vec::new());
    wtr.write_record(header).expect("writing to a Vec never fails");
    for row in rows {
        wtr.write_record(row).expect("writing to a Vec never fails");
    }
    let bytes = wtr.into_inner().expect("flushing a Vec never fails");
    String::from_utf8(bytes).expect("csv writer emits utf-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_fields_that_need_it() {
        let rows = vec![vec![
            "GB2QL5W4".to_owned(),
            "Canu, Will H.;Carlson, Caryn L.".to_owned(),
            "unread;adhd".to_owned(),
        ]];
        let out = render_csv(&["key", "authors", "tags"], &rows);
        // A field with a comma is quoted; a `;`-joined field without a comma is not.
        assert_eq!(
            out,
            "key,authors,tags\nGB2QL5W4,\"Canu, Will H.;Carlson, Caryn L.\",unread;adhd\n"
        );
    }
}
