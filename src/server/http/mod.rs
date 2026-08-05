//! Bounded one-request HTTP/1.x transport for the shared process.

mod request;
mod response;

pub(super) use request::{BodyError, Request, RequestError};
pub(super) use response::Response;
