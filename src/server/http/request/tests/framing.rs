use super::{super::BodyError, super::RequestError, parse_request};

#[test]
fn rejects_conflicting_or_ambiguous_request_framing() {
    for framing in [
        "Content-Length: 0\r\nTransfer-Encoding: chunked\r\n",
        "Content-Length: 0\r\nContent-Length: 0\r\n",
        "Content-Length: +1\r\n",
        "Transfer-Encoding: gzip\r\n",
        "Transfer-Encoding: gzip, chunked\r\n",
        "Transfer-Encoding: chunked, gzip\r\n",
        "Transfer-Encoding: chunked, chunked\r\n",
        "Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n",
    ] {
        let raw = format!("POST / HTTP/1.1\r\nHost: localhost\r\n{framing}\r\n");
        assert!(
            matches!(parse_request(raw.as_bytes()), Err(RequestError::Malformed)),
            "ambiguous framing was accepted: {framing:?}"
        );
    }
}

#[test]
fn rejects_invalid_http_field_names_without_trimming_them() {
    for field in [
        "Transfer-Encoding : chunked",
        " Transfer-Encoding: chunked",
        "Transfer Encoding: chunked",
        "Transfer@Encoding: chunked",
        ": chunked",
    ] {
        let raw = format!("POST / HTTP/1.1\r\nHost: localhost\r\n{field}\r\n\r\n");
        assert!(
            matches!(parse_request(raw.as_bytes()), Err(RequestError::Malformed)),
            "invalid field name was accepted: {field:?}"
        );
    }
}

#[test]
fn rejects_non_http_whitespace_in_framing_values() {
    let mut accepted = Vec::new();
    for whitespace in ['\u{000b}', '\u{000c}', '\u{00a0}'] {
        for framing in [
            format!("Content-Length:{whitespace}1"),
            format!("Transfer-Encoding:{whitespace}chunked"),
        ] {
            let raw = format!("POST / HTTP/1.1\r\nHost: localhost\r\n{framing}\r\n\r\n");
            if !matches!(parse_request(raw.as_bytes()), Err(RequestError::Malformed)) {
                accepted.push(framing);
            }
        }
    }
    assert!(
        accepted.is_empty(),
        "non-HTTP whitespace was accepted: {accepted:?}"
    );
}

#[test]
fn accepts_space_and_tab_as_framing_ows() {
    let mut length =
        parse_request(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: \t1\t \r\n\r\na")
            .expect("parse content length with HTTP OWS");
    assert_eq!(length.read_body(1).expect("read length body"), b"a");

    let mut chunked = parse_request(
        b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: \tchunked\t \r\n\r\n0\r\n\r\n",
    )
    .expect("parse transfer encoding with HTTP OWS");
    assert!(
        chunked
            .read_body(1)
            .expect("read empty chunked body")
            .is_empty()
    );
}

#[test]
fn accepts_one_exact_chunked_coding_with_bounded_chunks_and_trailers() {
    let raw = b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n0\r\nX-Trace: ok\r\n\r\n";
    let mut request = parse_request(raw).expect("parse exact chunked request");

    assert_eq!(request.read_body(16).expect("read chunked body"), b"Wiki");
}

#[test]
fn rejects_malformed_chunk_sizes_and_trailer_field_names() {
    for body in [
        " 4\r\nWiki\r\n0\r\n\r\n",
        "+4\r\nWiki\r\n0\r\n\r\n",
        "4x\r\nWiki\r\n0\r\n\r\n",
        "0\r\nBad Trailer: value\r\n\r\n",
        "0\r\nContent-Length : 4\r\n\r\n",
        "0\r\nContent-Length: 4\r\n\r\n",
        "0\r\nTransfer-Encoding: chunked\r\n\r\n",
    ] {
        let raw = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n{body}"
        );
        let mut request = parse_request(raw.as_bytes()).expect("parse chunked request head");
        assert!(
            matches!(request.read_body(16), Err(BodyError::Malformed)),
            "malformed chunk grammar was accepted: {body:?}"
        );
    }
}

#[test]
fn chunk_payloads_and_trailer_sections_keep_their_hard_bounds() {
    let mut oversized_chunk = parse_request(
        b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
    )
    .expect("parse oversized chunk request head");
    assert!(matches!(
        oversized_chunk.read_body(4),
        Err(BodyError::TooLarge)
    ));

    let trailer = "a".repeat(16 * 1024);
    let raw = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Trace: {trailer}\r\n\r\n"
    );
    let mut oversized_trailer =
        parse_request(raw.as_bytes()).expect("parse oversized trailer request head");
    assert!(matches!(
        oversized_trailer.read_body(4),
        Err(BodyError::TooLarge)
    ));
}
