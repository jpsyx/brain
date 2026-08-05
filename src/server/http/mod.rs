//! Bounded one-request HTTP/1.x transport for the shared process.

pub(in crate::server) mod deadline;
mod request;
mod response;

pub(super) use request::{BodyError, Request, RequestError};
pub(super) use response::Response;
