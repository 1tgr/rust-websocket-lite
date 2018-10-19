#![deny(missing_docs)]
#![deny(warnings)]

//! A fast, low-overhead WebSocket client.
//!
//! This library is optimised for receiving a high volume of messages over a long period. A key feature is that is makes
//! no memory allocations once the connection is set up and the initial messages have been sent and received; it reuses
//! a single pair of buffers, which are sized for the longest message seen so far.
//!
//! Only asynchronous access is provided at present. `native_tls` provides the TLS functionality for `wss://...` servers.

extern crate base64;
extern crate byteorder;
extern crate bytes;
extern crate futures;
extern crate httparse;
extern crate native_tls;
extern crate rand;
extern crate take_mut;
extern crate tokio_io;
extern crate tokio_tcp;
extern crate tokio_tls;
extern crate url;

use std::error;
use std::io::{self, Cursor, Read};
use std::mem;
use std::net::ToSocketAddrs;
use std::ops::Range;
use std::result;
use std::str::{self, Utf8Error};

use base64::display::Base64Display;
use bytes::{BufMut, Bytes, BytesMut};
use byteorder::{BigEndian, ReadBytesExt};
use futures::{Future, Stream};
use futures::future::{self, Either, IntoFuture};
use httparse::Response;
use native_tls::TlsConnector;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsConnectorExt;
use tokio_tcp::TcpStream;
use url::Url;

#[allow(deprecated)]
use tokio_io::codec::{Decoder, Encoder, Framed};

/// Represents errors that can be exposed by this crate.
pub type Error = Box<error::Error + Sync + Send + 'static>;

/// Represents results returned by the non-async functions in this crate.
pub type Result<T> = result::Result<T, Error>;

/// A type that is both `AsyncRead` and `AsyncWrite`, such as a network stream.
pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<S> AsyncReadWrite for S
where
    S: AsyncRead + AsyncWrite,
{
}

/// A text string or a block of binary data that can be sent or recevied over a WebSocket.
#[derive(Clone, Debug)]
pub struct Message {
    is_text: bool,
    data: Bytes,
}

impl Message {
    /// Creates a message from a `Bytes` object.
    ///
    /// The message can be tagged as text or binary. When the `is_text` is `true` this function validates the bytes in
    /// `data` and returns `Err` if they do not contain valid UTF-8 text.
    pub fn new(is_text: bool, data: Bytes) -> result::Result<Self, Utf8Error> {
        if is_text {
            str::from_utf8(&data)?;
        }

        Ok(Message { is_text, data })
    }

    /// Creates a text message from a `&str`.
    pub fn text(data: &str) -> Self {
        Message {
            is_text: true,
            data: data.into(),
        }
    }

    /// Creates a binary message from any type that can be converted to `Bytes`, such as `&[u8]` or `Vec<u8>`.
    pub fn binary<B: Into<Bytes>>(data: B) -> Self {
        Message {
            is_text: false,
            data: data.into(),
        }
    }

    /// Returns a reference to the data held in this message.
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// For text messages, return a reference to the text.
    pub fn as_text(&self) -> Option<&str> {
        if self.is_text {
            Some(unsafe { str::from_utf8_unchecked(&self.data) })
        } else {
            None
        }
    }
}

struct UpgradeCodec;

impl Decoder for UpgradeCodec {
    type Item = ();
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<()>> {
        let response_len = {
            let mut headers = [httparse::EMPTY_HEADER; 20];
            let mut response = Response::new(&mut headers);
            let status = response.parse(&src)?;
            if !status.is_complete() {
                return Ok(None);
            }

            // TODO: validate the server's response!
            status.unwrap()
        };

        src.advance(response_len);
        Ok(Some(()))
    }
}

impl Encoder for UpgradeCodec {
    type Item = ();
    type Error = Error;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<()> {
        unimplemented!()
    }
}

struct FrameHeader {
    fin: bool,
    opcode: u8,
    mask: Option<[u8; 4]>,
    len: usize,
}

macro_rules! try_eof {
    ($result: expr) => {{
        let result: result::Result<_, io::Error> = $result;
        match result {
            Ok(value) => value,
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return Ok(None);
                } else {
                    return Err(e.into());
                }
            }
        }
    }};
}

impl FrameHeader {
    fn validate(data: &[u8]) -> Result<Option<(Self, Range<usize>)>> {
        let mut c = Cursor::new(data);

        let (fin, opcode) = {
            let b = try_eof!(c.read_u8());

            let fin = match b & 0xf0 {
                0x00 => false,
                0x80 => true,
                _ => {
                    return Err("reserved bits are not supported".into());
                }
            };

            (fin, b & 0x0f)
        };

        let (mask, len) = {
            let b = try_eof!(c.read_u8());

            let len = match b & 0x7f {
                127 => try_eof!(c.read_u64::<BigEndian>()) as usize,
                126 => try_eof!(c.read_u16::<BigEndian>()) as usize,
                n => {
                    assert!(n < 126);
                    n as usize
                }
            };

            let mask = if b & 0x80 == 0 {
                None
            } else {
                let mut mask = [0; 4];
                try_eof!(c.read_exact(&mut mask));
                Some(mask)
            };

            (mask, len)
        };

        let data_start = c.position() as usize;
        let data_end = data_start + len;
        if data.len() < data_end {
            return Ok(None);
        }

        let header = FrameHeader {
            fin,
            opcode,
            mask,
            len,
        };

        Ok(Some((header, data_start..data_end)))
    }

    fn write_to(&self, dst: &mut BytesMut) {
        if !self.fin {
            assert_eq!(0, self.opcode);
        }

        dst.reserve(10 + self.len as usize);
        dst.put_u8((if self.fin { 0x80 } else { 0x00 }) | self.opcode);

        let mask_bit = if self.mask.is_some() { 0x80 } else { 0x00 };
        if self.len > 65535 {
            dst.put_u8(mask_bit | 127);
            dst.put_u64_be(self.len as u64);
        } else if self.len >= 126 {
            dst.put_u8(mask_bit | 126);
            dst.put_u16_be(self.len as u16);
        } else {
            dst.put_u8(mask_bit | self.len as u8);
        }

        if let Some(mask) = &self.mask {
            dst.reserve(4);
            dst.put_slice(mask);
        }
    }
}

/// Tokio codec for WebSocket messages.
pub struct MessageCodec {
    mask_buf: Bytes,
}

impl MessageCodec {
    fn new() -> Self {
        MessageCodec {
            mask_buf: Bytes::new(),
        }
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>> {
        let (header, data_range) = if let Some(tuple) = FrameHeader::validate(&src)? {
            tuple
        } else {
            return Ok(None);
        };

        assert!(header.fin);

        let is_text = if header.opcode == 1 {
            true
        } else {
            assert_eq!(header.opcode, 2);
            false
        };

        let data = src.split_to(data_range.end)
            .freeze()
            .slice(data_range.start, data_range.end);

        Ok(Some(Message::new(is_text, data)?))
    }
}

impl Encoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<()> {
        let mask: u32 = rand::random();
        let mask: [u8; 4] = unsafe { mem::transmute(mask) };

        let header = FrameHeader {
            fin: true,
            opcode: if item.is_text { 1 } else { 2 },
            mask: Some(mask),
            len: item.data.len(),
        };

        header.write_to(dst);

        let mask = mask.iter().cycle();

        let data = match item.data.try_mut() {
            Ok(mut data) => {
                for (b, &mask) in data.iter_mut().zip(mask) {
                    *b = *b ^ mask;
                }

                data.freeze()
            }

            Err(data) => {
                take_mut::take(&mut self.mask_buf, |mask_buf| {
                    let mut mask_buf = mask_buf
                        .try_mut()
                        .unwrap_or_else(|_old_mask_buf| BytesMut::new());

                    mask_buf.resize(data.len(), 0);

                    for (dest, (&src, &mask)) in mask_buf.iter_mut().zip(data.iter().zip(mask)) {
                        *dest = src ^ mask;
                    }

                    mask_buf.freeze()
                });

                self.mask_buf.clone()
            }
        };

        dst.put(data);
        Ok(())
    }
}

/// Exposes a `Sink` for sending WebSocket messages, and a `Stream` for receiving them.
#[allow(deprecated)]
pub type Client<S> = Framed<S, MessageCodec>;

/// Establishes a WebSocket connection.
///
/// `ws://...` and `wss://...` URLs are supported.
pub struct ClientBuilder {
    url: Url,
    key: Option<[u8; 16]>,
}

impl ClientBuilder {
    /// Creates a `ClientBuilder` that connects to a given WebSocket URL.
    pub fn new(url: Url) -> Self {
        ClientBuilder { url, key: None }
    }

    // Not pub - used by the tests
    #[cfg(test)]
    fn key(mut self, key: &[u8]) -> Self {
        let mut a = [0; 16];
        a.copy_from_slice(key);
        self.key = Some(a);
        self
    }

    /// Establish a connection to the WebSocket server.
    pub fn connect(
        self,
    ) -> impl Future<Item = Client<Box<AsyncReadWrite + Sync + Send + 'static>>, Error = Error>
    {
        self.url
            .to_socket_addrs()
            .map_err(Into::into)
            .and_then(|mut addrs| {
                addrs
                    .next()
                    .ok_or_else(|| "can't resolve host".to_owned().into())
            })
            .into_future()
            .and_then(|addr| TcpStream::connect(&addr).map_err(Into::into))
            .and_then(move |stream| {
                if self.url.scheme() == "wss" {
                    Either::A(
                        TlsConnector::builder()
                            .and_then(|builder| builder.build())
                            .map_err(Into::into)
                            .into_future()
                            .and_then(move |cx| {
                                cx.connect_async(self.url.domain().unwrap_or(""), stream)
                                    .map_err(Into::into)
                                    .map(|stream| {
                                        let b: Box<
                                            AsyncReadWrite + Sync + Send + 'static,
                                        > = Box::new(stream);
                                        (b, self)
                                    })
                            }),
                    )
                } else {
                    let b: Box<AsyncReadWrite + Sync + Send + 'static> = Box::new(stream);
                    Either::B(future::ok((b, self)))
                }
            })
            .and_then(|(stream, this)| this.connect_on(stream))
    }

    /// Take over an already established stream and use it to send and receive WebSocket messages.
    ///
    /// This method assumes that the TLS connection has already been established, if needed. It sends an HTTP
    /// `Connection: Upgrade` request and waits for an HTTP OK response before proceeding.
    pub fn connect_on<S: AsyncRead + AsyncWrite>(
        self,
        stream: S,
    ) -> impl Future<Item = Client<S>, Error = Error> {
        let key = self.key.unwrap_or_else(|| rand::random());
        tokio_io::io::write_all(
            stream,
            format!(
                "GET {path} HTTP/1.1\r\n\
                 Host: {host}\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Key: {key}\r\n\
                 Sec-WebSocket-Version: 13\r\n\
                 \r\n",
                path = self.url.path(),
                host = self.url.domain().unwrap_or(""),
                key = Base64Display::standard(&key)
            ),
        ).map_err(Into::into)
            .and_then(move |(stream, _request)| {
                #[allow(deprecated)]
                let framed = stream.framed(UpgradeCodec);

                framed.into_future().map_err(|(e, _framed)| e)
            })
            .and_then(move |(opt, framed)| {
                opt.ok_or_else(|| "no HTTP Upgrade response".to_owned())?;

                #[allow(deprecated)]
                let framed = Framed::from_parts(framed.into_parts(), MessageCodec::new());

                Ok(framed)
            })
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::io::{self, Cursor, Read, Write};
    use std::result;
    use std::str;

    use base64;
    use futures::{Future, Poll};
    use tokio_io::{AsyncRead, AsyncWrite};
    use url::Url;

    use super::ClientBuilder;

    type Result<T> = result::Result<T, super::Error>;

    pub struct ReadWritePair<R, W>(pub R, pub W);

    impl<R: Read, W> Read for ReadWritePair<R, W> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }

        fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
            self.0.read_to_end(buf)
        }

        fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
            self.0.read_to_string(buf)
        }

        fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
            self.0.read_exact(buf)
        }
    }

    impl<R, W: Write> Write for ReadWritePair<R, W> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.1.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.1.flush()
        }

        fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
            self.1.write_all(buf)
        }

        fn write_fmt(&mut self, fmt: fmt::Arguments) -> io::Result<()> {
            self.1.write_fmt(fmt)
        }
    }

    impl<R: AsyncRead, W> AsyncRead for ReadWritePair<R, W> {}

    impl<R, W: AsyncWrite> AsyncWrite for ReadWritePair<R, W> {
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            self.1.shutdown()
        }
    }

    #[test]
    fn can_connect_on() -> Result<()> {
        let request = "GET /stream:8000 HTTP/1.1\r\n\
                       Host: localhost\r\n\
                       Upgrade: websocket\r\n\
                       Connection: Upgrade\r\n\
                       Sec-WebSocket-Key: x3JJHMbDL1EzLkh9GBhXDw==\r\n\
                       Sec-WebSocket-Version: 13\r\n\
                       \r\n";

        let response = "HTTP/1.1 101 Switching Protocols\r\n\
                        Upgrade: websocket\r\n\
                        Connection: Upgrade\r\n\
                        Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
                        \r\n";

        let mut input = Cursor::new(&response[..]);
        let mut output = Cursor::new(Vec::new());
        ClientBuilder::new(Url::parse("ws://localhost/stream:8000")?)
            .key(&base64::decode(b"x3JJHMbDL1EzLkh9GBhXDw==")?)
            .connect_on(ReadWritePair(&mut input, &mut output))
            .wait()?;

        assert_eq!(request, str::from_utf8(&output.into_inner())?);
        Ok(())
    }
}
