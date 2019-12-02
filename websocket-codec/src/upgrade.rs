use std::result;
use std::str;

use base64;
use base64::display::Base64Display;
use bytes::{Buf, BytesMut};
use httparse::{self, Header, Response};
use sha1::{self, Sha1};
use tokio_util::codec::{Decoder, Encoder};

use crate::{Error, Result};

fn header<'a, 'header: 'a>(headers: &'a [Header<'header>], name: &'a str) -> result::Result<&'header [u8], String> {
    let header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("server didn't respond with {name} header", name = name))?;

    Ok(header.value)
}

fn validate(expected_ws_accept: &[u8; sha1::DIGEST_LENGTH], data: &[u8]) -> Result<Option<usize>> {
    let mut headers = [httparse::EMPTY_HEADER; 20];
    let mut response = Response::new(&mut headers);
    let status = response.parse(data)?;
    if !status.is_complete() {
        return Ok(None);
    }

    let response_len = status.unwrap();
    let code = response.code.unwrap();
    if code != 101 {
        return Err(format!("server responded with HTTP error {code}", code = code).into());
    }

    let ws_accept_header = header(response.headers, "Sec-WebSocket-Accept")?;
    let mut ws_accept = [0; sha1::DIGEST_LENGTH];
    base64::decode_config_slice(&ws_accept_header, base64::STANDARD, &mut ws_accept)?;
    if expected_ws_accept != &ws_accept {
        return Err(format!(
            "server responded with incorrect Sec-WebSocket-Accept header: expected {expected}, got {actual}",
            expected = Base64Display::with_config(expected_ws_accept, base64::STANDARD),
            actual = Base64Display::with_config(&ws_accept, base64::STANDARD),
        )
        .into());
    }

    Ok(Some(response_len))
}

/// Tokio decoder for parsing the server's response to the client's HTTP `Connection: Upgrade` request.
pub struct UpgradeCodec {
    ws_accept: [u8; sha1::DIGEST_LENGTH],
}

impl UpgradeCodec {
    /// Returns a new `UpgradeCodec` object.
    ///
    /// The `key` parameter provides the string passed to the server via the HTTP `Sec-WebSocket-Key` header.
    pub fn new(key: &str) -> Self {
        let mut s = Sha1::new();
        s.update(key.as_bytes());
        s.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        UpgradeCodec {
            ws_accept: s.digest().bytes(),
        }
    }
}

impl Decoder for UpgradeCodec {
    type Item = ();
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<()>> {
        if let Some(response_len) = validate(&self.ws_accept, &src)? {
            src.advance(response_len);
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
}

impl Encoder for UpgradeCodec {
    type Item = ();
    type Error = Error;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<()> {
        unimplemented!()
    }
}
