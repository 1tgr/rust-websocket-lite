use bytes::BytesMut;
use criterion::{criterion_group, criterion_main, Criterion};
use tokio_util::codec::{Decoder, Encoder};
use websocket_codec::MessageCodec;

fn bench_decode(c: &mut Criterion, id: &str, data: &[u8], use_mask: bool) {
    let mut codec = MessageCodec::with_masked_encode(use_mask);
    codec.decode(&mut BytesMut::from(data)).unwrap().unwrap();

    c.bench_function(id, move |b| {
        let data = BytesMut::from(data);
        b.iter(|| codec.decode(&mut data.clone()))
    });
}

fn bench_encode(c: &mut Criterion, id: &str, data: &[u8], use_mask: bool) {
    let mut codec = MessageCodec::with_masked_encode(use_mask);
    let message = codec.decode(&mut BytesMut::from(data)).unwrap().unwrap();

    let mut data = BytesMut::with_capacity(data.len());
    codec.encode(&message, &mut data).unwrap();

    c.bench_function(id, move |b| {
        b.iter(|| {
            data.clear();
            codec.encode(&message, &mut data.clone())
        })
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let tiny_bytes = include_bytes!("../../fuzz/corpus/custom/c9942e909a823b80df0728be2ba7a8e1689e69ef");
    let small_bytes = include_bytes!("../../fuzz/corpus/custom/16ed7b740770ec7c03d464e8cf9eb0d234a09e5f");
    let medium_bytes = include_bytes!("../../fuzz/corpus/custom/bce1016fd05e9d70328cf0845004f50df264f4e6");

    assert_eq!(tiny_bytes.len(), 2);
    assert_eq!(small_bytes.len(), 127);
    assert_eq!(medium_bytes.len(), 1028);

    bench_encode(c, "masked encode tiny", tiny_bytes, true);
    bench_encode(c, "nomask encode tiny", tiny_bytes, false);
    bench_encode(c, "masked encode small", small_bytes, true);
    bench_encode(c, "nomask encode small", small_bytes, false);
    bench_encode(c, "masked encode medium", medium_bytes, true);
    bench_encode(c, "nomask encode medium", medium_bytes, false);

    bench_decode(c, "masked decode tiny", tiny_bytes, true);
    bench_decode(c, "nomask decode tiny", tiny_bytes, false);
    bench_decode(c, "masked decode small", small_bytes, true);
    bench_decode(c, "nomask decode small", small_bytes, false);
    bench_decode(c, "masked decode medium", medium_bytes, true);
    bench_decode(c, "nomask decode medium", medium_bytes, false);
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
