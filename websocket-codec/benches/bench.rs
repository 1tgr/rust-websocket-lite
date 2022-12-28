#![warn(clippy::pedantic)]

use bytes::BytesMut;
use criterion::measurement::Measurement;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkGroup, Criterion};
use static_assertions::const_assert_eq;
use tokio_util::codec::{Decoder, Encoder};
use websocket_codec::protocol::FrameHeaderCodec;
use websocket_codec::MessageCodec;

fn bench_decode<M: Measurement, C, Item>(c: &mut BenchmarkGroup<M>, id: &str, data: &[u8], mut codec: C)
where
    C: Decoder<Item = Item> + Encoder<Item>,
{
    let item = codec
        .decode(&mut BytesMut::from(data))
        .unwrap_or_else(|_| panic!("didn't expect decode() to return an error"))
        .unwrap();

    let mut data = BytesMut::with_capacity(data.len());
    codec
        .encode(item, &mut data)
        .unwrap_or_else(|_| panic!("didn't expect encode() to return an error"));

    c.bench_function(id, move |b| {
        b.iter_with_setup(
            || {
                let mut src = BytesMut::new();
                src.extend_from_slice(&data);
                src.split() // eagerly promote the BytesMut to KIND_ARC so we don't allocate below
            },
            |mut src| codec.decode(black_box(&mut src)),
        );
    });
}

fn bench_encode<M: Measurement, C, Item>(c: &mut BenchmarkGroup<M>, id: &str, data: &[u8], mut codec: C)
where
    C: Decoder<Item = Item> + for<'a> Encoder<&'a Item>,
{
    let capacity = data.len();

    let item = codec
        .decode(&mut BytesMut::from(data))
        .unwrap_or_else(|_| panic!("didn't expect decode() to return an error"))
        .unwrap();

    let mut data = BytesMut::with_capacity(capacity);
    codec
        .encode(&item, &mut data)
        .unwrap_or_else(|_| panic!("didn't expect encode() to return an error"));

    let capacity = capacity.max(data.capacity());

    c.bench_function(id, move |b| {
        b.iter_with_setup(
            move || {
                let mut data = BytesMut::new();
                data.resize(capacity, 0);

                let mut dst = data.split(); // eagerly promote the BytesMut to KIND_ARC so we don't allocate below
                dst.clear();
                assert_eq!(dst.len(), 0);
                assert_eq!(dst.capacity(), capacity);
                dst
            },
            |mut dst| codec.encode(black_box(&item), black_box(&mut dst)),
        );
    });
}

const TINY_BYTES: &[u8] = include_bytes!("../../fuzz/corpus/custom/c9942e909a823b80df0728be2ba7a8e1689e69ef");
const SMALL_BYTES: &[u8] = include_bytes!("../../fuzz/corpus/custom/16ed7b740770ec7c03d464e8cf9eb0d234a09e5f");
const MEDIUM_BYTES: &[u8] = include_bytes!("../../fuzz/corpus/custom/bce1016fd05e9d70328cf0845004f50df264f4e6");

const_assert_eq!(TINY_BYTES.len(), 2);
const_assert_eq!(SMALL_BYTES.len(), 127);
const_assert_eq!(MEDIUM_BYTES.len(), 1028);

fn encode_benchmark(c: &mut Criterion) {
    let masked_codec = MessageCodec::with_masked_encode(true);
    let nomask_codec = MessageCodec::with_masked_encode(true);
    let mut c = c.benchmark_group("encode");

    bench_encode(&mut c, "masked message tiny", TINY_BYTES, masked_codec.clone());
    bench_encode(&mut c, "masked message small", SMALL_BYTES, masked_codec.clone());
    bench_encode(&mut c, "masked message medium", MEDIUM_BYTES, masked_codec);

    bench_encode(&mut c, "nomask message tiny", TINY_BYTES, nomask_codec.clone());
    bench_encode(&mut c, "nomask message small", SMALL_BYTES, nomask_codec.clone());
    bench_encode(&mut c, "nomask message medium", MEDIUM_BYTES, nomask_codec);

    bench_encode(&mut c, "header tiny", TINY_BYTES, FrameHeaderCodec);
    bench_encode(&mut c, "header small", SMALL_BYTES, FrameHeaderCodec);
    bench_encode(&mut c, "header medium", MEDIUM_BYTES, FrameHeaderCodec);

    c.finish();
}

fn decode_benchmark(c: &mut Criterion) {
    let masked_codec = MessageCodec::with_masked_encode(true);
    let nomask_codec = MessageCodec::with_masked_encode(true);
    let mut c = c.benchmark_group("decode");

    bench_decode(&mut c, "masked message tiny", TINY_BYTES, masked_codec.clone());
    bench_decode(&mut c, "masked message small", SMALL_BYTES, masked_codec.clone());
    bench_decode(&mut c, "masked message medium", MEDIUM_BYTES, masked_codec);

    bench_decode(&mut c, "nomask message tiny", TINY_BYTES, nomask_codec.clone());
    bench_decode(&mut c, "nomask message small", SMALL_BYTES, nomask_codec.clone());
    bench_decode(&mut c, "nomask message medium", MEDIUM_BYTES, nomask_codec);

    bench_decode(&mut c, "header tiny", TINY_BYTES, FrameHeaderCodec);
    bench_decode(&mut c, "header small", SMALL_BYTES, FrameHeaderCodec);
    bench_decode(&mut c, "header medium", MEDIUM_BYTES, FrameHeaderCodec);

    c.finish();
}

criterion_group!(benches, encode_benchmark, decode_benchmark);
criterion_main!(benches);
