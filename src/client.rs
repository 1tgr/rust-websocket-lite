use std::net::ToSocketAddrs;
use std::result;
use std::str;

use base64;
use futures::{Future, Stream};
use futures::future::{self, Either, IntoFuture};
use rand;
use tokio_codec::{Decoder, Encoder, Framed};
use tokio_io::{self, AsyncRead, AsyncWrite};
use tokio_tcp::TcpStream;
use url::{self, Url};

use super::{Error, MessageCodec};
use super::ssl;
use super::upgrade::UpgradeCodec;

/// A type that is both `AsyncRead` and `AsyncWrite`.
pub trait AsyncNetworkStream: AsyncRead + AsyncWrite {}

impl<S> AsyncNetworkStream for S
where
    S: AsyncRead + AsyncWrite,
{
}

/// Exposes a `Sink` for sending WebSocket messages, and a `Stream` for receiving them.
pub type Client<S> = Framed<S, MessageCodec>;

fn set_codec<T: AsyncRead + AsyncWrite, C1, C2: Encoder + Decoder>(framed: Framed<T, C1>, codec: C2) -> Framed<T, C2> {
    // TODO improve this? https://github.com/tokio-rs/tokio/issues/717
    let parts1 = framed.into_parts();
    let mut parts2 = Framed::new(parts1.io, codec).into_parts();
    parts2.read_buf = parts1.read_buf;
    parts2.write_buf = parts1.write_buf;
    Framed::from_parts(parts2)
}

/// Establishes a WebSocket connection.
///
/// `ws://...` and `wss://...` URLs are supported.
pub struct ClientBuilder {
    url: Url,
    key: Option<[u8; 16]>,
}

impl ClientBuilder {
    /// Creates a `ClientBuilder` that connects to a given WebSocket URL.
    ///
    /// This method returns an `Err` result if URL parsing fails.
    pub fn new(url: &str) -> result::Result<Self, url::ParseError> {
        Ok(Self::from_url(Url::parse(url)?))
    }

    /// Creates a `ClientBuilder` that connects to a given WebSocket URL.
    ///
    /// This method never fails as the URL has already been parsed.
    pub fn from_url(url: Url) -> Self {
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
    pub fn async_connect(
        self,
    ) -> impl Future<Item = Client<Box<AsyncNetworkStream + Sync + Send + 'static>>, Error = Error> {
        self.url
            .to_socket_addrs()
            .map_err(Into::into)
            .and_then(|mut addrs| addrs.next().ok_or_else(|| "can't resolve host".to_owned().into()))
            .into_future()
            .and_then(|addr| TcpStream::connect(&addr).map_err(Into::into))
            .and_then(move |stream| {
                if self.url.scheme() == "wss" {
                    let domain = self.url.domain().unwrap_or("").to_owned();
                    Either::A(ssl::wrap(domain, stream).map(move |stream| {
                        let b: Box<AsyncNetworkStream + Sync + Send + 'static> = Box::new(stream);
                        (b, self)
                    }))
                } else {
                    let b: Box<AsyncNetworkStream + Sync + Send + 'static> = Box::new(stream);
                    Either::B(future::ok((b, self)))
                }
            })
            .and_then(|(stream, this)| this.async_connect_on(stream))
    }

    /// Take over an already established stream and use it to send and receive WebSocket messages.
    ///
    /// This method assumes that the TLS connection has already been established, if needed. It sends an HTTP
    /// `Connection: Upgrade` request and waits for an HTTP OK response before proceeding.
    pub fn async_connect_on<S: AsyncRead + AsyncWrite>(
        self,
        stream: S,
    ) -> impl Future<Item = Client<S>, Error = Error> {
        let key_bytes = self.key.unwrap_or_else(rand::random);
        let mut key_base64 = [0; 24];
        assert_eq!(
            24,
            base64::encode_config_slice(&key_bytes, base64::STANDARD, &mut key_base64)
        );

        let key = str::from_utf8(&key_base64).unwrap();
        let upgrade_codec = UpgradeCodec::new(key);

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
                key = key,
            ),
        ).map_err(Into::into)
            .and_then(move |(stream, _request)| upgrade_codec.framed(stream).into_future().map_err(|(e, _framed)| e))
            .and_then(move |(opt, framed)| {
                opt.ok_or_else(|| "no HTTP Upgrade response".to_owned())?;
                Ok(set_codec(framed, MessageCodec::new()))
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
                       Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                       Sec-WebSocket-Version: 13\r\n\
                       \r\n";

        let response = "HTTP/1.1 101 Switching Protocols\r\n\
                        Upgrade: websocket\r\n\
                        Connection: Upgrade\r\n\
                        Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
                        \r\n";

        let mut input = Cursor::new(&response[..]);
        let mut output = Cursor::new(Vec::new());
        ClientBuilder::new("ws://localhost/stream:8000")?
            .key(&base64::decode(b"dGhlIHNhbXBsZSBub25jZQ==")?)
            .async_connect_on(ReadWritePair(&mut input, &mut output))
            .wait()?;

        assert_eq!(request, str::from_utf8(&output.into_inner())?);
        Ok(())
    }
}
