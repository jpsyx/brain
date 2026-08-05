use std::io::{BufReader, Cursor, Read as _};

use super::super::{LimitedLineError, read_limited_line};

#[test]
fn oversized_line_consumes_only_one_proof_byte_beyond_the_limit() {
    let mut source = vec![b'a'; 4096];
    source.extend_from_slice(b"\r\n");
    let mut reader = BufReader::with_capacity(source.len(), Cursor::new(source.clone()));

    assert!(matches!(
        read_limited_line(&mut reader, 128),
        Err(LimitedLineError::TooLarge)
    ));

    let mut remaining = Vec::new();
    reader
        .read_to_end(&mut remaining)
        .expect("read bytes beyond the bounded proof prefix");
    assert_eq!(remaining.len(), source.len() - 129);
}
