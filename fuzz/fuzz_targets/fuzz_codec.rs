#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use tokio_util::codec::Decoder;
use websocket_codec::{MessageCodec, Result};

fn run(data: &[u8]) -> Result<()> {
    let mut data = BytesMut::from(data);
    MessageCodec::client().decode(&mut data)?;
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(data);
});
