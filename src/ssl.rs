use futures::{Future, IntoFuture};
use tokio_io::{AsyncRead, AsyncWrite};

use super::Error;

#[cfg(all(feature = "ssl-native-tls", feature = "ssl-openssl"))]
compile_error!("Features ssl-native-tls and ssl-openssl can't be used at the same time");

#[cfg(feature = "ssl-native-tls")]
pub fn wrap<S: AsyncRead + AsyncWrite>(
    domain: String,
    stream: S,
) -> impl Future<Item = ::tokio_tls::TlsStream<S>, Error = Error> {
    use native_tls::TlsConnector;
    use tokio_tls::TlsConnectorExt;

    TlsConnector::builder()
        .and_then(|builder| builder.build())
        .map_err(Into::into)
        .into_future()
        .and_then(move |cx| cx.connect_async(&domain, stream).map_err(Into::into))
}

#[cfg(feature = "ssl-openssl")]
pub fn wrap<S: AsyncRead + AsyncWrite>(
    domain: String,
    stream: S,
) -> impl Future<Item = ::tokio_openssl::SslStream<S>, Error = Error> {
    use std::env;
    use std::fs::File;
    use std::io::Write;
    use std::sync::Mutex;

    use openssl::ssl::{SslConnector, SslMethod};
    use tokio_openssl::SslConnectorExt;

    SslConnector::builder(SslMethod::tls())
        .map_err(Into::into)
        .into_future()
        .and_then(|mut cx| {
            if let Ok(filename) = env::var("SSLKEYLOGFILE") {
                let file = Mutex::new(File::create(filename)?);
                cx.set_keylog_callback(move |_ssl, line| {
                    let mut file = file.lock().unwrap();
                    let _ = writeln!(&mut file, "{}", line);
                });
            }

            Ok(cx)
        })
        .and_then(move |cx| cx.build().connect_async(&domain, stream).map_err(Into::into))
}
