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

pub use self::inner::*;
