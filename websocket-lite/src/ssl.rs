#[cfg(feature = "__ssl-rustls")]
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    pin::Pin,
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(feature = "__ssl-rustls")]
use tokio_rustls::{rustls::ClientConfig, webpki::DNSNameRef};

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
    NativeTls(tokio_native_tls::TlsConnector),
    /// `rustls` TLS connector.
    #[cfg(feature = "__ssl-rustls")]
    Rustls(tokio_rustls::TlsConnector),
}

impl Debug for Connector {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Plain => f.write_str("Connector::Plain"),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => connector.fmt(f),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(_) => f.write_str("Connector::Rustls"),
        }
    }
}

/// A stream that might be protected with TLS.
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

impl Connector {
    /// Creates a new `Connector` with the underlying TLS library specified in the feature flags.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` when creating the underlying TLS connector fails.
    pub fn with_default_tls_config() -> Result<Self> {
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
            let mut config = ClientConfig::new();
            config
                .root_store
                .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            Ok(Self::Rustls(connector))
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
            Ok(Self::Rustls(connector))
        }
    }

    /// Wraps a given stream with a layer of TLS.
    ///
    /// # Errors
    ///
    /// This method returns an `Err` if the TLS handshake fails.
    pub async fn wrap<S: AsyncRead + AsyncWrite + Unpin>(&self, domain: &str, stream: S) -> Result<MaybeTlsStream<S>> {
        match self {
            Self::Plain => Ok(MaybeTlsStream::Plain(stream)),
            #[cfg(feature = "ssl-native-tls")]
            Self::NativeTls(connector) => Ok(MaybeTlsStream::NativeTls(connector.connect(domain, stream).await?)),
            #[cfg(feature = "__ssl-rustls")]
            Self::Rustls(connector) => Ok(MaybeTlsStream::Rustls(
                connector
                    .connect(DNSNameRef::try_from_ascii_str(domain)?, stream)
                    .await?,
            )),
        }
    }
}
