use std::fmt::{Debug, Formatter};
use std::io::{Read, Write};
use std::net::TcpStream as StdTcpStream;
use std::pin::Pin;
#[cfg(feature = "__ssl-rustls")]
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{fmt, io};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream as TokioTcpStream;

use crate::Result;

#[cfg(all(feature = "ssl-native-tls", feature = "__ssl-rustls"))]
compile_error!("Only one TLS backend may be enabled at once");
#[cfg(all(feature = "ssl-rustls-webpki-roots", feature = "ssl-rustls-native-roots"))]
compile_error!("Only one of ssl-rustls-webpki-roots and ssl-rustls-native-roots may be enabled at once");

/// A reusable TLS connector for wrapping streams.
#[derive(Clone)]
pub enum Connector {
    /// Plain (non-TLS) connector.
    Plain,
    /// `native-tls` TLS connector.
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(native_tls::TlsConnector),
    /// `rustls` TLS connector.
    #[cfg(feature = "__ssl-rustls")]
    Rustls(Arc<rustls::ClientConfig>),
}

impl Debug for Connector {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => f.write_str("Connector::Plain"),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => connector.fmt(f),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(_) => f.write_str("Connector::Rustls"),
        }
    }
}

/// A reusable TLS connector for wrapping streams.
#[derive(Clone)]
pub enum AsyncConnector {
    /// Plain (non-TLS) connector.
    Plain,
    /// `native-tls` async TLS connector.
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(tokio_native_tls::TlsConnector),
    /// `rustls` async TLS connector.
    #[cfg(feature = "__ssl-rustls")]
    Rustls(tokio_rustls::TlsConnector),
}

impl Debug for AsyncConnector {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => f.write_str("AsyncConnector::Plain"),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => connector.fmt(f),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(_) => f.write_str("AsyncConnector::Rustls"),
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum AsyncMaybeTlsStreamInner {
    Plain(TokioTcpStream),
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(tokio_native_tls::TlsStream<TokioTcpStream>),
    #[cfg(feature = "__ssl-rustls")]
    Rustls(tokio_rustls::client::TlsStream<TokioTcpStream>),
}

/// An async stream that might be protected with TLS.
pub struct AsyncMaybeTlsStream {
    inner: AsyncMaybeTlsStreamInner,
}

impl AsyncRead for AsyncMaybeTlsStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            AsyncMaybeTlsStreamInner::Plain(ref mut s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "ssl-native-tls")]
            AsyncMaybeTlsStreamInner::NativeTls(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "__ssl-rustls")]
            AsyncMaybeTlsStreamInner::Rustls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for AsyncMaybeTlsStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().inner {
            AsyncMaybeTlsStreamInner::Plain(ref mut s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "ssl-native-tls")]
            AsyncMaybeTlsStreamInner::NativeTls(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "__ssl-rustls")]
            AsyncMaybeTlsStreamInner::Rustls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            AsyncMaybeTlsStreamInner::Plain(ref mut s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "ssl-native-tls")]
            AsyncMaybeTlsStreamInner::NativeTls(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "__ssl-rustls")]
            AsyncMaybeTlsStreamInner::Rustls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            AsyncMaybeTlsStreamInner::Plain(ref mut s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "ssl-native-tls")]
            AsyncMaybeTlsStreamInner::NativeTls(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "__ssl-rustls")]
            AsyncMaybeTlsStreamInner::Rustls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum MaybeTlsStreamInner {
    Plain(StdTcpStream),
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(native_tls::TlsStream<StdTcpStream>),
    #[cfg(feature = "__ssl-rustls")]
    Rustls(rustls::StreamOwned<rustls::ClientSession, StdTcpStream>),
}

/// A stream that might be protected with TLS.
pub struct MaybeTlsStream {
    inner: MaybeTlsStreamInner,
}

impl Read for MaybeTlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inner {
            MaybeTlsStreamInner::Plain(ref mut s) => s.read(buf),
            #[cfg(feature = "ssl-native-tls")]
            MaybeTlsStreamInner::NativeTls(ref mut s) => s.read(buf),
            #[cfg(feature = "__ssl-rustls")]
            MaybeTlsStreamInner::Rustls(ref mut s) => s.read(buf),
        }
    }
}

impl Write for MaybeTlsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.inner {
            MaybeTlsStreamInner::Plain(ref mut s) => s.write(buf),
            #[cfg(feature = "ssl-native-tls")]
            MaybeTlsStreamInner::NativeTls(ref mut s) => s.write(buf),
            #[cfg(feature = "__ssl-rustls")]
            MaybeTlsStreamInner::Rustls(ref mut s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.inner {
            MaybeTlsStreamInner::Plain(ref mut s) => s.flush(),
            #[cfg(feature = "ssl-native-tls")]
            MaybeTlsStreamInner::NativeTls(ref mut s) => s.flush(),
            #[cfg(feature = "__ssl-rustls")]
            MaybeTlsStreamInner::Rustls(ref mut s) => s.flush(),
        }
    }
}

impl Connector {
    /// Creates a new `Connector` with the underlying TLS library specified in the feature flags.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` when creating the underlying TLS connector fails.
    ///
    /// # Panics
    ///
    /// With feature `ssl-rustls-native-roots`, this method panics if the root certificate store is empty.
    #[allow(clippy::unnecessary_wraps)]
    pub fn new_with_default_tls_config() -> Result<Self> {
        #[cfg(not(feature = "__ssl"))]
        {
            Ok(Self::Plain)
        }
        #[cfg(feature = "ssl-native-tls")]
        {
            Ok(Self::NativeTls(native_tls::TlsConnector::new()?))
        }
        #[cfg(feature = "ssl-rustls-webpki-roots")]
        {
            let mut config = rustls::ClientConfig::new();
            config
                .root_store
                .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
            Ok(Self::Rustls(Arc::new(config)))
        }
        #[cfg(feature = "ssl-rustls-native-roots")]
        {
            let mut config = rustls::ClientConfig::new();
            config.root_store = match rustls_native_certs::load_native_certs() {
                Ok(store) | Err((Some(store), _)) => store,
                Err((None, err)) => return Err(err.into()),
            };
            assert!(!config.root_store.is_empty(), "no CA certificates found");
            Ok(Self::Rustls(Arc::new(config)))
        }
    }

    #[allow(clippy::match_wildcard_for_single_variants)]
    #[allow(clippy::unnecessary_wraps)]
    #[allow(unused_variables)]
    pub(crate) fn wrap(self, domain: &str, stream: StdTcpStream) -> Result<MaybeTlsStream> {
        let inner = match self {
            Self::Plain => MaybeTlsStreamInner::Plain(stream),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => MaybeTlsStreamInner::NativeTls(connector.connect(domain, stream)?),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(client_config) => {
                let session =
                    rustls::ClientSession::new(&client_config, webpki::DNSNameRef::try_from_ascii_str(domain)?);
                MaybeTlsStreamInner::Rustls(rustls::StreamOwned::new(session, stream))
            }
        };

        Ok(MaybeTlsStream { inner })
    }
}

impl AsyncConnector {
    /// Creates a new async `Connector` with the underlying TLS library specified in the feature flags.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` when creating the underlying TLS connector fails.
    ///
    /// # Panics
    ///
    /// With feature `ssl-rustls-native-roots`, this method panics if the root certificate store is empty.
    #[allow(clippy::unnecessary_wraps)]
    pub fn new_with_default_tls_config() -> Result<Self> {
        #[cfg(not(feature = "__ssl"))]
        {
            Ok(Self::Plain)
        }
        #[cfg(feature = "ssl-native-tls")]
        {
            Ok(Self::NativeTls(native_tls::TlsConnector::new()?.into()))
        }
        #[cfg(feature = "ssl-rustls-webpki-roots")]
        {
            let mut config = rustls::ClientConfig::new();
            config
                .root_store
                .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            Ok(Self::Rustls(connector))
        }
        #[cfg(feature = "ssl-rustls-native-roots")]
        {
            let mut config = rustls::ClientConfig::new();
            config.root_store = match rustls_native_certs::load_native_certs() {
                Ok(store) | Err((Some(store), _)) => store,
                Err((None, err)) => return Err(err.into()),
            };
            assert!(!config.root_store.is_empty(), "no CA certificates found");
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            Ok(Self::Rustls(connector))
        }
    }

    #[allow(clippy::match_wildcard_for_single_variants)]
    #[allow(clippy::unnecessary_wraps)]
    #[allow(unused_variables)]
    pub(crate) async fn wrap(self, domain: &str, stream: TokioTcpStream) -> Result<AsyncMaybeTlsStream> {
        let inner = match self {
            Self::Plain => AsyncMaybeTlsStreamInner::Plain(stream),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => AsyncMaybeTlsStreamInner::NativeTls(connector.connect(domain, stream).await?),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(connector) => AsyncMaybeTlsStreamInner::Rustls(
                connector
                    .connect(webpki::DNSNameRef::try_from_ascii_str(domain)?, stream)
                    .await?,
            ),
        };

        Ok(AsyncMaybeTlsStream { inner })
    }
}
