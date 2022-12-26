use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcpStream};
use std::result;
use std::str;

use futures_util::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::TcpStream as TokioTcpStream,
};
use tokio_util::codec::{Decoder, Framed};
use url::{self, Url};
use websocket_codec::UpgradeCodec;

use crate::sync;
use crate::{AsyncClient, Client, MessageCodec, Result};

fn replace_codec<T, C1, C2>(framed: Framed<T, C1>, codec: C2) -> Framed<T, C2>
where
    T: AsyncRead + AsyncWrite,
{
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
    url.socket_addrs(|| None)?
        .into_iter()
        .next()
        .ok_or_else(|| "can't resolve host".to_owned().into())
}

fn make_key(key: Option<[u8; 16]>, key_base64: &mut [u8; 24]) -> &str {
    let key_bytes = key.unwrap_or_else(rand::random);
    assert_eq!(
        24,
        base64::encode_config_slice(&key_bytes, base64::STANDARD, key_base64)
    );

    str::from_utf8(key_base64).unwrap()
}

fn build_request(url: &Url, key: &str, headers: &[(String, String)]) -> String {
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
         Sec-WebSocket-Version: 13\r\n",
        key = key
    );

    for (name, value) in headers {
        writeok!(s, "{name}: {value}\r\n", name = name, value = value);
    }

    writeok!(s, "\r\n");
    s
}

/// Establishes a WebSocket connection.
///
/// `ws://...` and `wss://...` URLs are supported.
pub struct ClientBuilder {
    url: Url,
    key: Option<[u8; 16]>,
    headers: Vec<(String, String)>,
}

impl ClientBuilder {
    /// Creates a `ClientBuilder` that connects to a given WebSocket URL.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if URL parsing fails.
    pub fn new(url: &str) -> result::Result<Self, url::ParseError> {
        Ok(Self::from_url(Url::parse(url)?))
    }

    /// Creates a `ClientBuilder` that connects to a given WebSocket URL.
    ///
    /// This method never fails as the URL has already been parsed.
    #[must_use]
    pub fn from_url(url: Url) -> Self {
        ClientBuilder {
            url,
            key: None,
            headers: Vec::new(),
        }
    }

    /// Adds an extra HTTP header for client
    ///
    pub fn add_header(&mut self, name: String, value: String) {
        self.headers.push((name, value));
    }

    /// Establishes a connection to the WebSocket server.
    ///
    /// `wss://...` URLs are not supported by this method. Use `async_connect` if you need to be able to handle
    /// both `ws://...` and `wss://...` URLs.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if connecting to the server fails.
    pub async fn async_connect_insecure(self) -> Result<AsyncClient<TokioTcpStream>> {
        let addr = resolve(&self.url)?;
        let stream = TokioTcpStream::connect(&addr).await?;
        self.async_connect_on(stream).await
    }

    /// Establishes a connection to the WebSocket server.
    ///
    /// `wss://...` URLs are not supported by this method. Use `connect` if you need to be able to handle
    /// both `ws://...` and `wss://...` URLs.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if connecting to the server fails.
    pub fn connect_insecure(self) -> Result<Client<StdTcpStream>> {
        let addr = resolve(&self.url)?;
        let stream = StdTcpStream::connect(&addr)?;
        self.connect_on(stream)
    }

    /// Establishes a connection to the WebSocket server.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if connecting to the server fails.
    #[cfg(any(feature = "ssl-native-tls", feature = "ssl-openssl"))]
    pub async fn async_connect(
        self,
    ) -> Result<AsyncClient<Box<dyn crate::AsyncNetworkStream + Sync + Send + Unpin + 'static>>> {
        let addr = resolve(&self.url)?;
        let stream = TokioTcpStream::connect(&addr).await?;

        let stream: Box<dyn crate::AsyncNetworkStream + Sync + Send + Unpin + 'static> = if self.url.scheme() == "wss" {
            let domain = self.url.domain().unwrap_or("").to_owned();
            let stream = crate::ssl::async_wrap(domain, stream).await?;
            Box::new(stream)
        } else {
            Box::new(stream)
        };

        self.async_connect_on(stream).await
    }

    /// Establishes a connection to the WebSocket server.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if connecting to the server fails.
    #[cfg(any(feature = "ssl-native-tls", feature = "ssl-openssl"))]
    pub fn connect(self) -> Result<Client<Box<dyn crate::NetworkStream + Sync + Send + 'static>>> {
        let addr = resolve(&self.url)?;
        let stream = StdTcpStream::connect(&addr)?;

        let stream: Box<dyn crate::NetworkStream + Sync + Send + 'static> = if self.url.scheme() == "wss" {
            let domain = self.url.domain().unwrap_or("");
            let stream = crate::ssl::wrap(domain, stream)?;
            Box::new(stream)
        } else {
            Box::new(stream)
        };

        self.connect_on(stream)
    }

    /// Takes over an already established stream and uses it to send and receive WebSocket messages.
    ///
    /// This method assumes that the TLS connection has already been established, if needed. It sends an HTTP
    /// `Connection: Upgrade` request and waits for an HTTP OK response before proceeding.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if writing or reading from the stream fails.
    pub async fn async_connect_on<S: AsyncRead + AsyncWrite + Unpin>(self, mut stream: S) -> Result<AsyncClient<S>> {
        let mut key_base64 = [0; 24];
        let key = make_key(self.key, &mut key_base64);
        let upgrade_codec = UpgradeCodec::new(key);
        let request = build_request(&self.url, key, &self.headers);
        AsyncWriteExt::write_all(&mut stream, request.as_bytes()).await?;

        let (opt, framed) = upgrade_codec.framed(stream).into_future().await;
        opt.ok_or_else(|| "no HTTP Upgrade response".to_owned())??;
        Ok(replace_codec(framed, MessageCodec::client()))
    }

    /// Takes over an already established stream and uses it to send and receive WebSocket messages.
    ///
    /// This method assumes that the TLS connection has already been established, if needed. It sends an HTTP
    /// `Connection: Upgrade` request and waits for an HTTP OK response before proceeding.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` result if writing or reading from the stream fails.
    pub fn connect_on<S: Read + Write>(self, mut stream: S) -> Result<Client<S>> {
        let mut key_base64 = [0; 24];
        let key = make_key(self.key, &mut key_base64);
        let upgrade_codec = UpgradeCodec::new(key);
        let request = build_request(&self.url, key, &self.headers);
        Write::write_all(&mut stream, request.as_bytes())?;

        let mut framed = sync::Framed::new(stream, upgrade_codec);
        framed.receive()?.ok_or_else(|| "no HTTP Upgrade response".to_owned())?;
        Ok(framed.replace_codec(MessageCodec::client()))
    }

    // Not pub - used by the tests
    #[cfg(test)]
    fn key(mut self, key: &[u8]) -> Self {
        let mut a = [0; 16];
        a.copy_from_slice(key);
        self.key = Some(a);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::io::{self, Cursor, Read, Write};
    use std::pin::Pin;
    use std::result;
    use std::str;
    use std::task::{Context, Poll};

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

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

        fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
            self.1.write_fmt(fmt)
        }
    }

    impl<R: AsyncRead + Unpin, W: Unpin> AsyncRead for ReadWritePair<R, W> {
        fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
        }
    }

    impl<R: Unpin, W: AsyncWrite + Unpin> AsyncWrite for ReadWritePair<R, W> {
        fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.get_mut().1).poll_write(cx, buf)
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().1).poll_flush(cx)
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().1).poll_shutdown(cx)
        }
    }

    static REQUEST: &str = "GET /stream?query HTTP/1.1\r\n\
                            Host: localhost:8000\r\n\
                            Upgrade: websocket\r\n\
                            Connection: Upgrade\r\n\
                            Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                            Sec-WebSocket-Version: 13\r\n\
                            \r\n";

    static RESPONSE: &str = "HTTP/1.1 101 Switching Protocols\r\n\
                             Upgrade: websocket\r\n\
                             Connection: Upgrade\r\n\
                             sec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
                             \r\n";

    #[tokio::test]
    async fn can_async_connect_on() -> Result<()> {
        let mut input = Cursor::new(RESPONSE);
        let mut output = Vec::new();

        ClientBuilder::new("ws://localhost:8000/stream?query")?
            .key(&base64::decode(b"dGhlIHNhbXBsZSBub25jZQ==")?)
            .async_connect_on(ReadWritePair(&mut input, &mut output))
            .await
            .unwrap();

        assert_eq!(REQUEST, str::from_utf8(&output)?);
        Ok(())
    }

    #[test]
    fn can_connect_on() -> Result<()> {
        let mut input = Cursor::new(RESPONSE);
        let mut output = Vec::new();

        ClientBuilder::new("ws://localhost:8000/stream?query")?
            .key(&base64::decode(b"dGhlIHNhbXBsZSBub25jZQ==")?)
            .connect_on(ReadWritePair(&mut input, &mut output))?;

        assert_eq!(REQUEST, str::from_utf8(&output)?);
        Ok(())
    }
}
