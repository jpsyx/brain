use std::io::Write as _;
use std::net::{TcpListener, TcpStream};

use super::{Request, RequestError};

mod deadline;
mod framing;
mod limits;

fn parse_request(raw: &[u8]) -> Result<Request, RequestError> {
    let (mut client, server) = tcp_pair();
    client.write_all(raw).expect("write raw request");
    Request::read(server)
}

fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
    let client = TcpStream::connect(listener.local_addr().expect("test address"))
        .expect("connect test client");
    let (server, _) = listener.accept().expect("accept test client");
    (client, server)
}
