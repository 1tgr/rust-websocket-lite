use std::fmt;
use std::io::{Read, Write};
use std::net::{self, SocketAddr, ToSocketAddrs};
use std::result;
use std::str;

use base64;
use futures::{Future, Stream};
use futures::future::{self, Either, IntoFuture};
use rand;
use tokio_codec::{Decoder, Encoder, Framed};
use tokio_io::{self, AsyncRead, AsyncWrite};
use tokio_tcp;
use url::{self, Url};
use websocket_codec::UpgradeCodec;

use crate::{AsyncClient, AsyncNetworkStream, Client, Error, MessageCodec, NetworkStream, Result};
use crate::ssl;
use crate::sync;

fn replace_codec<T: AsyncRead + AsyncWrite, C1, C2: Encoder + Decoder>(
    framed: Framed<T, C1>,
    codec: C2,
) -> Framed<T, C2> {
    // TODO improve this? https://github.com/tokio-rs/tokio/issues/717
    let parts1 = framed.into_parts();
    let mut parts2 = Framed::new(parts1.io, codec).into_parts();
    parts2.read_buf = parts1.read_buf;
    parts2.write_buf = parts1.write_buf;
    Framed::from_parts(parts2)
}

macro_rules! writeok {
    ($dst:expr, $($arg:tt)*) => {
        let _ = fmt::Write::write_fmt(&mut $dst, format_args!($($arg)*));
    }
}

fn resolve(url: &Url) -> Result<SocketAddr> {
    let mut addrs = url.to_socket_addrs()?;
    addrs.next().ok_or_else(|| "can't resolve host".to_owned().into())
}

fn make_key(key: Option<[u8; 16]>, key_base64: &mut [u8; 24]) -> &str {
    let key_bytes = key.unwrap_or_else(rand::random);
    assert_eq!(
        24,
        base64::encode_config_slice(&key_bytes, base64::STANDARD, key_base64)
    );

    str::from_utf8(key_base64).unwrap()
}

fn build_request(url: &Url, key: &str) -> String {
    let mut s = String::new();
    writeok!(s, "GET {path}", path = url.path());
    if let Some(query) = url.query() {
        writeok!(s, "?{query}", query = query);
    }

    s += " HTTP/1.1\r\n";

    if let Some(host) = url.host() {
        writeok!(s, "Host: {host}", host = host);
        if let Some(port) = url.port_or_known_default() {
            writeok!(s, ":{port}", port = port);
        }

        s += "\r\n";
    }

    writeok!(
        s,
        "Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n",
        key = key
    );
    s
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

    /// Establishes a connection to the WebSocket server.
    ///
    /// `wss://...` URLs are not supported by this method. Use `async_connect` if you need to be able to handle
    /// both `ws://...` and `wss://...` URLs.
    pub fn async_connect_insecure(self) -> impl Future<Item = AsyncClient<tokio_tcp::TcpStream>, Error = Error> {
        resolve(&self.url)
            .into_future()
            .and_then(|addr| tokio_tcp::TcpStream::connect(&addr).map_err(Into::into))
            .and_then(|stream| self.async_connect_on(stream))
    }

    /// Establishes a connection to the WebSocket server.
    ///
    /// `wss://...` URLs are not supported by this method. Use `connect` if you need to be able to handle
    /// both `ws://...` and `wss://...` URLs.
    pub fn connect_insecure(self) -> Result<Client<net::TcpStream>> {
        let addr = resolve(&self.url)?;
        let stream = net::TcpStream::connect(&addr)?;
        self.connect_on(stream)
    }

    /// Establishes a connection to the WebSocket server.
    pub fn async_connect(
        self,
    ) -> impl Future<Item = AsyncClient<Box<AsyncNetworkStream + Sync + Send + 'static>>, Error = Error> {
        resolve(&self.url)
            .into_future()
            .and_then(|addr| tokio_tcp::TcpStream::connect(&addr).map_err(Into::into))
            .and_then(move |stream| {
                if self.url.scheme() == "wss" {
                    let domain = self.url.domain().unwrap_or("").to_owned();
                    Either::A(ssl::async_wrap(domain, stream).map(move |stream| {
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

    /// Establishes a connection to the WebSocket server.
    pub fn connect(self) -> Result<Client<Box<NetworkStream + Sync + Send + 'static>>> {
        let addr = resolve(&self.url)?;
        let stream = net::TcpStream::connect(&addr)?;

        let stream = if self.url.scheme() == "wss" {
            let domain = self.url.domain().unwrap_or("");
            let stream = ssl::wrap(domain, stream)?;
            let b: Box<NetworkStream + Sync + Send + 'static> = Box::new(stream);
            b
        } else {
            let b: Box<NetworkStream + Sync + Send + 'static> = Box::new(stream);
            b
        };

        self.connect_on(stream)
    }

    /// Takes over an already established stream and uses it to send and receive WebSocket messages.
    ///
    /// This method assumes that the TLS connection has already been established, if needed. It sends an HTTP
    /// `Connection: Upgrade` request and waits for an HTTP OK response before proceeding.
    pub fn async_connect_on<S: AsyncRead + AsyncWrite>(
        self,
        stream: S,
    ) -> impl Future<Item = AsyncClient<S>, Error = Error> {
        let mut key_base64 = [0; 24];
        let key = make_key(self.key, &mut key_base64);
        let upgrade_codec = UpgradeCodec::new(key);
        tokio_io::io::write_all(stream, build_request(&self.url, key))
            .map_err(Into::into)
            .and_then(move |(stream, _request)| upgrade_codec.framed(stream).into_future().map_err(|(e, _framed)| e))
            .and_then(move |(opt, framed)| {
                opt.ok_or_else(|| "no HTTP Upgrade response".to_owned())?;
                Ok(replace_codec(framed, MessageCodec::new()))
            })
    }

    /// Takes over an already established stream and uses it to send and receive WebSocket messages.
    ///
    /// This method assumes that the TLS connection has already been established, if needed. It sends an HTTP
    /// `Connection: Upgrade` request and waits for an HTTP OK response before proceeding.
    pub fn connect_on<S: Read + Write>(self, mut stream: S) -> Result<Client<S>> {
        let mut key_base64 = [0; 24];
        let key = make_key(self.key, &mut key_base64);
        let upgrade_codec = UpgradeCodec::new(key);
        stream.write_all(build_request(&self.url, key).as_ref())?;

        let mut framed = sync::Framed::new(stream, upgrade_codec);
        framed.receive()?.ok_or_else(|| "no HTTP Upgrade response".to_owned())?;
        Ok(framed.replace_codec(MessageCodec::new()))
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

    use crate::ClientBuilder;

    type Result<T> = result::Result<T, crate::Error>;

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

    static REQUEST: &'static str = "GET /stream?query HTTP/1.1\r\n\
                                    Host: localhost:8000\r\n\
                                    Upgrade: websocket\r\n\
                                    Connection: Upgrade\r\n\
                                    Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                                    Sec-WebSocket-Version: 13\r\n\
                                    \r\n";

    static RESPONSE: &'static str = "HTTP/1.1 101 Switching Protocols\r\n\
                                     Upgrade: websocket\r\n\
                                     Connection: Upgrade\r\n\
                                     Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
                                     \r\n";

    #[test]
    fn can_async_connect_on() -> Result<()> {
        let mut input = Cursor::new(&RESPONSE[..]);
        let mut output = Cursor::new(Vec::new());
        ClientBuilder::new("ws://localhost:8000/stream?query")?
            .key(&base64::decode(b"dGhlIHNhbXBsZSBub25jZQ==")?)
            .async_connect_on(ReadWritePair(&mut input, &mut output))
            .wait()?;

        assert_eq!(REQUEST, str::from_utf8(&output.into_inner())?);
        Ok(())
    }

    #[test]
    fn can_connect_on() -> Result<()> {
        let mut input = Cursor::new(&RESPONSE[..]);
        let mut output = Cursor::new(Vec::new());
        ClientBuilder::new("ws://localhost:8000/stream?query")?
            .key(&base64::decode(b"dGhlIHNhbXBsZSBub25jZQ==")?)
            .connect_on(ReadWritePair(&mut input, &mut output))?;

        assert_eq!(REQUEST, str::from_utf8(&output.into_inner())?);
        Ok(())
    }
}
