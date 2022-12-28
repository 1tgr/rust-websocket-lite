use std::fmt::Write;
use std::{result, str};

use base64::display::Base64Display;
use bytes::{Buf, BytesMut};
use httparse::{Header, Response};
use sha1::Sha1;
use tokio_util::codec::{Decoder, Encoder};

use crate::{Error, Result};

type Sha1Digest = [u8; sha1::DIGEST_LENGTH];

fn build_ws_accept(key: &str) -> Sha1Digest {
    let mut s = Sha1::new();
    s.update(key.as_bytes());
    s.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    s.digest().bytes()
}

fn header<'a, 'header: 'a>(headers: &'a [Header<'header>], name: &'a str) -> result::Result<&'header [u8], String> {
    let header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("server didn't respond with {name} header", name = name))?;

    Ok(header.value)
}

fn validate_server_response(expected_ws_accept: &Sha1Digest, data: &[u8]) -> Result<Option<usize>> {
    let mut headers = [httparse::EMPTY_HEADER; 20];
    let mut response = Response::new(&mut headers);
    let status = response.parse(data)?;
    if !status.is_complete() {
        return Ok(None);
    }

    let response_len = status.unwrap();
    let code = response.code.unwrap();
    if code != 101 {
        let mut error_message = format!("server responded with HTTP error {code}", code = code);

        if let Some(reason) = response.reason {
            write!(error_message, ": {:?}", reason).expect("formatting reason failed");
        }

        return Err(error_message.into());
    }

    let ws_accept_header = header(response.headers, "Sec-WebSocket-Accept")?;
    let mut ws_accept = Sha1Digest::default();
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

fn contains_ignore_ascii_case(mut haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }

    while haystack.len() >= needle.len() {
        if haystack[..needle.len()].eq_ignore_ascii_case(needle) {
            return true;
        }

        haystack = &haystack[1..];
    }

    false
}

/// A client's opening handshake.
pub struct ClientRequest {
    ws_accept: Sha1Digest,
}

impl ClientRequest {
    /// Parses the client's opening handshake.
    ///
    /// # Errors
    ///
    /// This method fails when a header required for the WebSocket protocol is missing in the handshake.
    pub fn parse<'a, F>(header: F) -> Result<Self>
    where
        F: Fn(&'static str) -> Option<&'a str> + 'a,
    {
        let header = |name| header(name).ok_or_else(|| format!("client didn't provide {name} header", name = name));

        let check_header = |name, expected| {
            let actual = header(name)?;
            if actual.eq_ignore_ascii_case(expected) {
                Ok(())
            } else {
                Err(format!(
                    "client provided incorrect {name} header: expected {expected}, got {actual}",
                    name = name,
                    expected = expected,
                    actual = actual
                ))
            }
        };

        let check_header_contains = |name, expected: &str| {
            let actual = header(name)?;
            if contains_ignore_ascii_case(actual.as_bytes(), expected.as_bytes()) {
                Ok(())
            } else {
                Err(format!(
                    "client provided incorrect {name} header: expected string containing {expected}, got {actual}",
                    name = name,
                    expected = expected,
                    actual = actual
                ))
            }
        };

        check_header("Upgrade", "websocket")?;
        check_header_contains("Connection", "Upgrade")?;
        check_header("Sec-WebSocket-Version", "13")?;

        let key = header("Sec-WebSocket-Key")?;
        let ws_accept = build_ws_accept(key);
        Ok(Self { ws_accept })
    }

    /// Copies the value that the client expects to see in the server's `Sec-WebSocket-Accept` header into a `String`.
    pub fn ws_accept_buf(&self, s: &mut String) {
        base64::encode_config_buf(&self.ws_accept, base64::STANDARD, s);
    }

    /// Returns the value that the client expects to see in the server's `Sec-WebSocket-Accept` header.
    #[must_use]
    pub fn ws_accept(&self) -> String {
        base64::encode_config(&self.ws_accept, base64::STANDARD)
    }
}

/// Tokio decoder for parsing the server's response to the client's HTTP `Connection: Upgrade` request.
pub struct UpgradeCodec {
    ws_accept: Sha1Digest,
}

impl UpgradeCodec {
    /// Returns a new `UpgradeCodec` object.
    ///
    /// The `key` parameter provides the string passed to the server via the HTTP `Sec-WebSocket-Key` header.
    #[must_use]
    pub fn new(key: &str) -> Self {
        UpgradeCodec {
            ws_accept: build_ws_accept(key),
        }
    }
}

impl Decoder for UpgradeCodec {
    type Item = ();
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<()>> {
        if let Some(response_len) = validate_server_response(&self.ws_accept, src)? {
            src.advance(response_len);
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
}

impl Encoder<()> for UpgradeCodec {
    type Error = Error;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<()> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use crate::upgrade::contains_ignore_ascii_case;

    #[test]
    fn does_not_contain() {
        assert!(!contains_ignore_ascii_case(b"World", b"hello"));
    }

    #[test]
    fn contains_exact() {
        assert!(contains_ignore_ascii_case(b"Hello", b"hello"));
    }

    #[test]
    fn contains_substring() {
        assert!(contains_ignore_ascii_case(b"Hello World", b"hello"));
    }
}
