//! Fixed connection-closing HTTP responses.

pub(in crate::server) struct Response {
    status: u16,
    content_type: Option<&'static str>,
    body: Vec<u8>,
}

impl Response {
    pub(in crate::server) fn text(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type: Some("text/plain; charset=utf-8"),
            body: body.into(),
        }
    }

    pub(in crate::server) fn html(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type: Some("text/html; charset=utf-8"),
            body: body.into(),
        }
    }

    pub(in crate::server) fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type: Some("application/json"),
            body: body.into(),
        }
    }

    pub(in crate::server) fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: None,
            body: Vec::new(),
        }
    }

    pub(in crate::server) fn write_to(
        &self,
        stream: &mut impl std::io::Write,
    ) -> std::io::Result<()> {
        write!(
            stream,
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.status,
            reason(self.status),
            self.body.len()
        )?;
        if let Some(content_type) = self.content_type {
            write!(stream, "Content-Type: {content_type}\r\n")?;
        }
        stream.write_all(b"\r\n")?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Response",
    }
}
