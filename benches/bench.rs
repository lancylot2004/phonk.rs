use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use phonk::executor::Executor;
use phonk::phonk;
use phonk_helpers::decode;
use std::hint::black_box;
use std::thread;

struct ChunkedExecutor {
    workers: usize,
}

impl Executor for ChunkedExecutor {
    fn execute<F>(&self, range: core::ops::Range<usize>, job: F)
    where
        F: Fn(usize, usize) + Sync,
    {
        let start = range.start;
        let end = range.end;
        if start >= end {
            return;
        }

        let workers = self.workers.max(1).min(end - start);
        let chunk = (end - start).div_ceil(workers);

        thread::scope(|scope| {
            for worker in 0..workers {
                let from = start + worker * chunk;
                let to = (from + chunk).min(end);
                if from < to {
                    let job_ref = &job;
                    scope.spawn(move || job_ref(from, to));
                }
            }
        });
    }
}

const ASSET_PATH: &str = "tests/assets/violin-in-cafe-440.mp3";
const BATCH_SIZE: usize = 9_600;
const STEP_SIZE: usize = 2_400;

fn bench_phonk(c: &mut Criterion) {
    let (samples_by_channel, sample_rate) = decode(ASSET_PATH);
    let samples = samples_by_channel
        .first()
        .expect("decoded audio has no channels");

    let frames = ((samples.len() - BATCH_SIZE) / STEP_SIZE + 1) as u64;

    let mut group = c.benchmark_group("phonk_violin_in_cafe_440");
    group.throughput(criterion::Throughput::Elements(frames));

    group.bench_function("serial", |b| {
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

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let executor = ChunkedExecutor { workers };

    group.bench_function("parallel", |b| {
        b.iter_batched(
            || {
                let mut detector = phonk!(BATCH_SIZE, sample_rate, 20, 8_000).unwrap();
                detector.push_samples(&samples[..BATCH_SIZE - STEP_SIZE]);
                detector
            },
            |mut detector| {
                for chunk in samples[BATCH_SIZE - STEP_SIZE..].chunks(STEP_SIZE) {
                    detector.push_samples(chunk);
                    black_box(detector.run_parallel(&executor));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_phonk);
criterion_main!(benches);
