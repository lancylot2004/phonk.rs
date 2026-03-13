use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use phonk::phonk;
use phonk_helpers::decode;

const ASSET_PATH: &str = "tests/assets/violin-in-cafe-440.mp3";
const BATCH_SIZE: usize = 9_600;
const STEP_SIZE: usize = 2_400;

fn bench_phonk(c: &mut Criterion) {
    let (samples_by_channel, sample_rate) = decode(ASSET_PATH);
    let samples = samples_by_channel
        .first()
        .expect("decoded audio has no channels");

    let mut group = c.benchmark_group("phonk_violin_in_cafe_440");

    group.bench_function("single_frame_run", |b| {
        b.iter_batched(
            || {
                let mut detector = phonk!(BATCH_SIZE, sample_rate, 20, 8_000).unwrap();
                detector.push_samples(&samples[..BATCH_SIZE - STEP_SIZE]);
                detector
            },
            |mut detector| {
                detector.push_samples(&samples[BATCH_SIZE - STEP_SIZE..BATCH_SIZE]);
                black_box(detector.run());
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("full_clip_sweep", |b| {
        b.iter_batched(
            || {
                let mut detector = phonk!(BATCH_SIZE, sample_rate, 20, 8_000).unwrap();
                detector.push_samples(&samples[..BATCH_SIZE - STEP_SIZE]);
                detector
            },
            |mut detector| {
                for chunk in samples[BATCH_SIZE - STEP_SIZE..].chunks(STEP_SIZE) {
                    detector.push_samples(chunk);
                    black_box(detector.run());
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_phonk);
criterion_main!(benches);
