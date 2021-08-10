use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Read, Write};
use std::pin::Pin;
#[cfg(feature = "__ssl-rustls")]
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(feature = "__ssl-rustls")]
use tokio_rustls::{
    rustls::{ClientConfig, ClientSession, StreamOwned},
    webpki::DNSNameRef,
};

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
    /// `native-tls` async TLS connector.
    #[cfg(feature = "ssl-native-tls")]
    NativeTlsAsync(tokio_native_tls::TlsConnector),
    /// `native-tls` TLS connector.
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(native_tls::TlsConnector),
    /// `rustls` async TLS connector.
    #[cfg(feature = "__ssl-rustls")]
    RustlsAsync(tokio_rustls::TlsConnector),
    /// `rustls` TLS connector.
    #[cfg(feature = "__ssl-rustls")]
    Rustls(Arc<ClientConfig>),
}

impl Debug for Connector {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Plain => f.write_str("Connector::Plain"),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTlsAsync(connector) => connector.fmt(f),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => connector.fmt(f),
            #[cfg(feature = "__ssl-rustls")]
            Self::RustlsAsync(_) => f.write_str("Connector::RustlsAsync"),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(_) => f.write_str("Connector::Rustls"),
        }
    }
}

/// An async stream that might be protected with TLS.
pub enum MaybeTlsStream<S> {
    /// Unencrypted socket stream.
    Plain(S),
    /// Encrypted socket stream using `native-tls`.
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(tokio_native_tls::TlsStream<S>),
    /// Encrypted socket stream using `rustls`.
    #[cfg(feature = "__ssl-rustls")]
    Rustls(tokio_rustls::client::TlsStream<S>),
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for MaybeTlsStream<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for MaybeTlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), std::io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), std::io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// A stream that might be protected with TLS.
pub enum SyncMaybeTlsStream<S: Read + Write + Sized> {
    /// Unencrypted socket stream.
    Plain(S),
    /// Encrypted socket stream using `native-tls`.
    #[cfg(feature = "ssl-native-tls")]
    NativeTls(native_tls::TlsStream<S>),
    /// Encrypted socket stream using `rustls`.
    #[cfg(feature = "__ssl-rustls")]
    Rustls(StreamOwned<ClientSession, S>),
}

impl<S: Read + Write> Read for SyncMaybeTlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(ref mut s) => s.read(buf),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(ref mut s) => s.read(buf),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(ref mut s) => s.read(buf),
        }
    }
}

impl<S: Read + Write> Write for SyncMaybeTlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(ref mut s) => s.write(buf),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(ref mut s) => s.write(buf),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(ref mut s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(ref mut s) => s.flush(),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(ref mut s) => s.flush(),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(ref mut s) => s.flush(),
        }
    }
}

impl Connector {
    /// Creates a new async `Connector` with the underlying TLS library specified in the feature flags.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` when creating the underlying TLS connector fails.
    pub fn new_async_with_default_tls_config() -> Result<Self> {
        #[cfg(not(feature = "__ssl"))]
        {
            Ok(Self::Plain)
        }
        #[cfg(feature = "ssl-native-tls")]
        {
            Ok(Self::NativeTlsAsync(native_tls::TlsConnector::new()?.into()))
        }
        #[cfg(feature = "ssl-rustls-webpki-roots")]
        {
            let mut config = ClientConfig::new();
            config
                .root_store
                .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            Ok(Self::RustlsAsync(connector))
        }
        #[cfg(feature = "ssl-rustls-native-roots")]
        {
            let mut config = ClientConfig::new();
            config.root_store = match rustls_native_certs::load_native_certs() {
                Ok(store) | Err((Some(store), _)) => store,
                Err((None, err)) => return Err(err.into()),
            };
            if config.root_store.is_empty() {
                panic!("no CA certificates found");
            }
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            Ok(Self::RustlsAsync(connector))
        }
    }

    /// Creates a new `Connector` with the underlying TLS library specified in the feature flags.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` when creating the underlying TLS connector fails.
    pub fn new_sync_with_default_tls_config() -> Result<Self> {
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
            let mut config = ClientConfig::new();
            config
                .root_store
                .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
            Ok(Self::Rustls(Arc::new(config)))
        }
        #[cfg(feature = "ssl-rustls-native-roots")]
        {
            let mut config = ClientConfig::new();
            config.root_store = match rustls_native_certs::load_native_certs() {
                Ok(store) | Err((Some(store), _)) => store,
                Err((None, err)) => return Err(err.into()),
            };
            if config.root_store.is_empty() {
                panic!("no CA certificates found");
            }
            Ok(Self::Rustls(Arc::new(config)))
        }
    }

    /// Wraps a given async stream with a layer of TLS.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` if the TLS handshake fails.
    ///
    /// # Panics
    ///
    /// This method panics when attempting to wrap with a sync TLS connector.
    #[allow(clippy::match_wildcard_for_single_variants)]
    pub async fn wrap<S: AsyncRead + AsyncWrite + Unpin>(&self, domain: &str, stream: S) -> Result<MaybeTlsStream<S>> {
        match self {
            Self::Plain => Ok(MaybeTlsStream::Plain(stream)),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTlsAsync(connector) => Ok(MaybeTlsStream::NativeTls(connector.connect(domain, stream).await?)),
            #[cfg(feature = "__ssl-rustls")]
            Self::RustlsAsync(connector) => Ok(MaybeTlsStream::Rustls(
                connector
                    .connect(DNSNameRef::try_from_ascii_str(domain)?, stream)
                    .await?,
            )),
            _ => panic!("Cannot wrap async stream with sync TLS connector"),
        }
    }

    /// Wraps a given stream with a layer of TLS.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` if the TLS handshake fails.
    ///
    /// # Panics
    /// This method panics when attempting to wrap with an async TLS connector.
    #[allow(clippy::match_wildcard_for_single_variants)]
    pub fn wrap_sync<S: 'static + Read + Write + Debug + Send + Sync>(
        &self,
        domain: &str,
        stream: S,
    ) -> Result<SyncMaybeTlsStream<S>> {
        match self {
            Self::Plain => Ok(SyncMaybeTlsStream::Plain(stream)),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => Ok(SyncMaybeTlsStream::NativeTls(connector.connect(domain, stream)?)),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(client_config) => {
                let session = ClientSession::new(client_config, DNSNameRef::try_from_ascii_str(domain)?);

                Ok(SyncMaybeTlsStream::Rustls(StreamOwned::new(session, stream)))
            }
            _ => panic!("Cannot wrap sync stream with async TLS connector"),
        }
    }
}
