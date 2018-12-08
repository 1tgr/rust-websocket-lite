#[cfg(all(feature = "ssl-native-tls", feature = "ssl-openssl"))]
compile_error!("Features ssl-native-tls and ssl-openssl can't be used at the same time");

#[cfg(feature = "ssl-native-tls")]
mod inner {
    use std::fmt::Debug;
    use std::io::{Read, Write};

    use futures::{Future, IntoFuture};
    use native_tls::TlsConnector;
    use tokio_io::{AsyncRead, AsyncWrite};
    use tokio_tls::TlsConnectorExt;

    use crate::{Error, Result};

    pub fn async_wrap<S: AsyncRead + AsyncWrite>(
        domain: String,
        stream: S,
    ) -> impl Future<Item = ::tokio_tls::TlsStream<S>, Error = Error> {
        TlsConnector::builder()
            .and_then(|builder| builder.build())
            .map_err(Into::into)
            .into_future()
            .and_then(move |cx| cx.connect_async(&domain, stream).map_err(Into::into))
    }

    pub fn wrap<S: Read + Write + Debug + 'static>(domain: &str, stream: S) -> Result<::native_tls::TlsStream<S>> {
        use native_tls::TlsConnector;

        let builder = TlsConnector::builder()?;
        let cx = builder.build()?;
        Ok(cx.connect(domain, stream)?)
    }
}

#[cfg(feature = "ssl-openssl")]
mod inner {
    use std::env;
    use std::fmt::Debug;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::sync::Mutex;

    use futures::{Future, IntoFuture};
    use openssl::ssl::{SslConnector, SslConnectorBuilder, SslMethod};
    use tokio_io::{AsyncRead, AsyncWrite};
    use tokio_openssl::SslConnectorExt;

    use {Error, Result};

    fn configure(cx: &mut SslConnectorBuilder) -> Result<()> {
        if let Ok(filename) = env::var("SSLKEYLOGFILE") {
            let file = Mutex::new(File::create(filename)?);
            cx.set_keylog_callback(move |_ssl, line| {
                let mut file = file.lock().unwrap();
                let _ = writeln!(&mut file, "{}", line);
            });
        }

        Ok(())
    }

    pub fn async_wrap<S: AsyncRead + AsyncWrite>(
        domain: String,
        stream: S,
    ) -> impl Future<Item = ::tokio_openssl::SslStream<S>, Error = Error> {
        SslConnector::builder(SslMethod::tls())
            .map_err(Into::into)
            .into_future()
            .and_then(|mut cx| {
                configure(&mut cx)?;
                Ok(cx)
            })
            .and_then(move |cx| cx.build().connect_async(&domain, stream).map_err(Into::into))
    }

    pub fn wrap<S: Read + Write>(
        domain: &str,
        stream: S,
    ) -> impl Future<Item = ::openssl::SslStream<S>, Error = Error> {
        let mut cx = SslConnector::builder(SslMethod::tls())?;
        configure(&mut cx)?;
        Ok(cx.build().connect(domain, stream)?)
    }
}

pub use self::inner::*;
