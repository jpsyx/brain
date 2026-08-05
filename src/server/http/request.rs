//! Bounded request-head and request-body parsing.

use std::io::{BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use super::Response;

const HEAD_LIMIT_BYTES: usize = 16 * 1024;
const CHUNK_LINE_LIMIT_BYTES: usize = 128;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

pub(in crate::server) struct Request {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: BodyKind,
    expect_continue: bool,
    reader: BufReader<TcpStream>,
}

impl Request {
    pub(in crate::server) fn read(stream: TcpStream) -> Result<Self, RequestError> {
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let mut reader = BufReader::new(stream);
        let mut total = 0;
        let request_line = read_head_line(&mut reader, &mut total)?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next().ok_or(RequestError::Malformed)?;
        let url = parts.next().ok_or(RequestError::Malformed)?;
        let version = parts.next().ok_or(RequestError::Malformed)?;
        if parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
            return Err(RequestError::Malformed);
        }

        let mut headers = Vec::new();
        loop {
            let line = read_head_line(&mut reader, &mut total)?;
            if line.is_empty() {
                break;
            }
            let (name, value) = line.split_once(':').ok_or(RequestError::Malformed)?;
            let name = name.trim();
            if name.is_empty() {
                return Err(RequestError::Malformed);
            }
            headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
        }

        let content_length = unique_content_length(&headers)?;
        let chunked = headers.iter().any(|(name, value)| {
            name == "transfer-encoding"
                && value
                    .split(',')
                    .any(|coding| coding.trim().eq_ignore_ascii_case("chunked"))
        });
        let body = if chunked {
            BodyKind::Chunked
        } else if let Some(length) = content_length {
            BodyKind::Length(length)
        } else {
            BodyKind::Empty
        };
        let expect_continue = headers
            .iter()
            .any(|(name, value)| name == "expect" && value.eq_ignore_ascii_case("100-continue"));

        Ok(Self {
            method: method.to_owned(),
            url: url.to_owned(),
            headers,
            body,
            expect_continue,
            reader,
        })
    }

    pub(in crate::server) fn method(&self) -> &str {
        &self.method
    }

    pub(in crate::server) fn url(&self) -> &str {
        &self.url
    }

    #[allow(dead_code)]
    pub(in crate::server) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub(in crate::server) fn read_body(&mut self, limit: usize) -> Result<Vec<u8>, BodyError> {
        if self.expect_continue {
            self.reader
                .get_mut()
                .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
            self.reader.get_mut().flush()?;
        }
        match self.body {
            BodyKind::Empty => Ok(Vec::new()),
            BodyKind::Length(length) => read_exact_body(&mut self.reader, length, limit),
            BodyKind::Chunked => read_chunked_body(&mut self.reader, limit),
        }
    }

    pub(in crate::server) fn write_response(&mut self, response: &Response) -> std::io::Result<()> {
        response.write_to(self.reader.get_mut())
    }
}

#[derive(Debug)]
pub(in crate::server) enum RequestError {
    Io(std::io::Error),
    Malformed,
    TooLarge,
}

impl From<std::io::Error> for RequestError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub(in crate::server) enum BodyError {
    Io(std::io::Error),
    Malformed,
    TooLarge,
}

impl From<std::io::Error> for BodyError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy)]
enum BodyKind {
    Empty,
    Length(usize),
    Chunked,
}

fn read_head_line(
    reader: &mut BufReader<TcpStream>,
    total: &mut usize,
) -> Result<String, RequestError> {
    let remaining = HEAD_LIMIT_BYTES
        .checked_sub(*total)
        .ok_or(RequestError::TooLarge)?;
    let mut line = read_limited_line(reader, remaining).map_err(|error| match error {
        LimitedLineError::Io(error) => RequestError::Io(error),
        LimitedLineError::Malformed => RequestError::Malformed,
        LimitedLineError::TooLarge => RequestError::TooLarge,
    })?;
    *total = total
        .checked_add(line.len())
        .ok_or(RequestError::TooLarge)?;
    line.truncate(line.len() - 2);
    String::from_utf8(line).map_err(|_| RequestError::Malformed)
}

fn unique_content_length(headers: &[(String, String)]) -> Result<Option<usize>, RequestError> {
    let mut found = None;
    for value in headers
        .iter()
        .filter(|(name, _)| name == "content-length")
        .map(|(_, value)| value)
    {
        let length = value
            .parse::<usize>()
            .map_err(|_| RequestError::Malformed)?;
        if found
            .replace(length)
            .is_some_and(|previous| previous != length)
        {
            return Err(RequestError::Malformed);
        }
    }
    Ok(found)
}

fn read_exact_body(
    reader: &mut BufReader<TcpStream>,
    length: usize,
    limit: usize,
) -> Result<Vec<u8>, BodyError> {
    if length > limit {
        let mut proof = vec![0_u8; limit.saturating_add(1)];
        reader.read_exact(&mut proof)?;
        return Err(BodyError::TooLarge);
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

fn read_chunked_body(
    reader: &mut BufReader<TcpStream>,
    limit: usize,
) -> Result<Vec<u8>, BodyError> {
    let mut body = Vec::new();
    loop {
        let mut line =
            read_limited_line(reader, CHUNK_LINE_LIMIT_BYTES).map_err(body_line_error)?;
        line.truncate(line.len() - 2);
        let size = std::str::from_utf8(&line)
            .ok()
            .and_then(|line| line.split(';').next())
            .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
            .ok_or(BodyError::Malformed)?;
        if size == 0 {
            consume_trailers(reader)?;
            return Ok(body);
        }
        if body
            .len()
            .checked_add(size)
            .is_none_or(|total| total > limit)
        {
            return Err(BodyError::TooLarge);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        let mut ending = [0_u8; 2];
        reader.read_exact(&mut ending)?;
        if ending != *b"\r\n" {
            return Err(BodyError::Malformed);
        }
    }
}

fn consume_trailers(reader: &mut BufReader<TcpStream>) -> Result<(), BodyError> {
    let mut total = 0;
    loop {
        let remaining = HEAD_LIMIT_BYTES
            .checked_sub(total)
            .ok_or(BodyError::TooLarge)?;
        let line = read_limited_line(reader, remaining).map_err(body_line_error)?;
        total = total.checked_add(line.len()).ok_or(BodyError::TooLarge)?;
        if line == b"\r\n" {
            return Ok(());
        }
    }
}

fn body_line_error(error: LimitedLineError) -> BodyError {
    match error {
        LimitedLineError::Io(error) => BodyError::Io(error),
        LimitedLineError::Malformed => BodyError::Malformed,
        LimitedLineError::TooLarge => BodyError::TooLarge,
    }
}

#[derive(Debug)]
enum LimitedLineError {
    Io(std::io::Error),
    Malformed,
    TooLarge,
}

fn read_limited_line(
    reader: &mut impl std::io::BufRead,
    limit: usize,
) -> Result<Vec<u8>, LimitedLineError> {
    let proof_limit = limit.saturating_add(1);
    let mut line = Vec::with_capacity(limit.min(1024));
    loop {
        let available = reader.fill_buf().map_err(LimitedLineError::Io)?;
        if available.is_empty() {
            return Err(LimitedLineError::Malformed);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let available_end = newline.map_or(available.len(), |index| index + 1);
        let take = available_end.min(proof_limit.saturating_sub(line.len()));
        line.extend_from_slice(&available[..take]);
        reader.consume(take);

        if line.len() > limit {
            return Err(LimitedLineError::TooLarge);
        }
        if newline.is_some() {
            return if line.ends_with(b"\r\n") {
                Ok(line)
            } else {
                Err(LimitedLineError::Malformed)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor, Read as _};

    use super::{LimitedLineError, read_limited_line};

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
}
