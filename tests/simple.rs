use phonk::executor::Executor;
use phonk::phonk;
use phonk_helpers::decode;
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

#[test]
fn detection_serial() {
    let (samples_by_channel, sample_rate) = decode("tests/assets/fork-440.mp3");
    let samples = samples_by_channel
        .first()
        .expect("decoded audio has no channels");
    let mut phonk = phonk!(9_600, sample_rate, 20, 8_000).unwrap();
    let (initial, samples) = samples.split_at(9_600 - 2_400);
    phonk.push_samples(initial);

    for (index, chunk) in samples.chunks(2_400).enumerate() {
        phonk.push_samples(chunk);
        let pitch = phonk.run();
        let pitch_string = match pitch {
            Some(pitch) => format!("Pitch: {pitch:.2} Hz"),
            None => "None".to_string(),
        };
        let chunk_start = index * 2_400;
        println!("Offset: {chunk_start:>6}, {pitch_string}");
    }
}

#[test]
fn detection_parallel() {
    let (samples_by_channel, sample_rate) = decode("tests/assets/fork-440.mp3");
    let samples = samples_by_channel
        .first()
        .expect("decoded audio has no channels");

    const BATCH_SIZE: usize = 9_600;
    const STEP_SIZE: usize = 2_400;

    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let executor = ChunkedExecutor { workers };

    let mut phonk = phonk!(BATCH_SIZE, sample_rate, 20, 8_000).unwrap();
    let (initial, samples) = samples.split_at(BATCH_SIZE - STEP_SIZE);
    phonk.push_samples(initial);

    for (index, chunk) in samples.chunks(STEP_SIZE).enumerate() {
        phonk.push_samples(chunk);
        let pitch = phonk.run_parallel(&executor);
        let pitch_string = match pitch {
            Some(pitch) => format!("Pitch: {pitch:.2} Hz"),
            None => "None".to_string(),
        };
        let chunk_start = index * STEP_SIZE;
        println!("Offset: {chunk_start:>6}, {pitch_string}");
    }
}
