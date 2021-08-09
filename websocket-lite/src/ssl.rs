#[cfg(all(feature = "ssl-native-tls", feature = "ssl-openssl"))]
compile_error!("Features ssl-native-tls and ssl-openssl can't be used at the same time");

#[cfg(feature = "ssl-native-tls")]
mod inner {
    use std::fmt::Debug;
    use std::io::{Read, Write};

    use native_tls::{HandshakeError, TlsConnector};
    use tokio::io::{AsyncRead, AsyncWrite};

    use crate::{Error, Result};

    pub async fn async_wrap<S: AsyncRead + AsyncWrite + Unpin>(
        domain: &str,
        stream: S,
    ) -> Result<tokio_native_tls::TlsStream<S>> {
        let builder = TlsConnector::builder();
        let cx = builder.build()?;
        Ok(tokio_native_tls::TlsConnector::from(cx).connect(domain, stream).await?)
    }

    pub fn wrap<S: Read + Write + Debug + 'static>(domain: &str, stream: S) -> Result<::native_tls::TlsStream<S>> {
        let builder = TlsConnector::builder();
        let cx = builder.build()?;
        cx.connect(domain, stream).map_err(|e| {
            if let HandshakeError::Failure(e) = e {
                Error::from(e)
            } else {
                Error::from(e.to_string())
            }
        })
    }
}

#[cfg(feature = "ssl-openssl")]
mod inner {
    use std::io::{Read, Write};
    use std::pin::Pin;

    use openssl::ssl::{SslConnector, SslMethod};
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio_openssl::SslStream;

    use crate::Result;

    pub async fn async_wrap<S: AsyncRead + AsyncWrite + Unpin>(domain: &str, stream: S) -> Result<SslStream<S>> {
        let ssl = SslConnector::builder(SslMethod::tls())?
            .build()
            .configure()?
            .into_ssl(domain)?;
        let mut stream = SslStream::new(ssl, stream)?;
        Pin::new(&mut stream).connect().await?;
        Ok(stream)
    }

    pub fn wrap<S: Read + Write>(domain: &str, stream: S) -> Result<openssl::ssl::SslStream<S>> {
        let ssl = SslConnector::builder(SslMethod::tls())?
            .build()
            .configure()?
            .into_ssl(domain)?;
        let mut stream = openssl::ssl::SslStream::new(ssl, stream)?;
        stream.connect()?;
        Ok(stream)
    }
}

#[cfg(any(feature = "ssl-rustls-webpki-roots", feature = "ssl-rustls-native-roots"))]
mod inner {
    use std::fmt::Debug;
    use std::io::{Read, Write};
    use std::sync::Arc;

    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio_rustls::{rustls, webpki::DNSNameRef, TlsConnector};

    use crate::Result;

    #[cfg(feature = "ssl-rustls-webpki-roots")]
    fn get_client_config() -> Arc<rustls::ClientConfig> {
        let mut config = rustls::ClientConfig::new();
        config
            .root_store
            .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
        Arc::new(config)
    }

    #[cfg(feature = "ssl-rustls-native-roots")]
    fn get_client_config() -> Arc<rustls::ClientConfig> {
        let mut config = rustls::ClientConfig::new();
        config.root_store = match rustls_native_certs::load_native_certs() {
            Ok(store) | Err((Some(store), _)) => store,
            Err((None, err)) => Err(err).expect("cannot access native cert store"),
        };
        if config.root_store.is_empty() {
            panic!("no CA certificates found");
        }
        Arc::new(config)
    }

    pub async fn async_wrap<S: AsyncRead + AsyncWrite + Unpin>(
        domain: &str,
        stream: S,
    ) -> Result<tokio_rustls::client::TlsStream<S>> {
        let connector = TlsConnector::from(get_client_config());
        let tls_stream = connector
            .connect(DNSNameRef::try_from_ascii_str(domain)?, stream)
            .await?;

        Ok(tls_stream)
    }

    pub fn wrap<S: Read + Write + Debug + 'static>(
        domain: &str,
        stream: S,
    ) -> Result<rustls::StreamOwned<rustls::ClientSession, S>> {
        let session = rustls::ClientSession::new(&get_client_config(), DNSNameRef::try_from_ascii_str(domain)?);
        Ok(rustls::StreamOwned::new(session, stream))
    }
}

pub use self::inner::*;
