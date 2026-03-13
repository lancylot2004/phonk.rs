#![no_std]

use crate::executor::Executor;
use crate::ring::Ring;

pub mod executor;
mod ring;

/// Constructs a [Phonk] with correctly derived const parameters (`W = (N + 63) / 64`,
/// `L = N / 2`), where `N`, `W`, and `L` are the batch size, number of 64-bit words needed for the
/// given batch size, and number of lags to compute, respectively. The macro also performs
/// compile-time validation of the input parameters.
///
/// * [batch_size] - Number of samples to process in each batch. Must be a multiple of 2 and greater
/// than 1. Scaling this parameter will impact performance linearly, and the minimum detectable
/// frequency is inversely proportional to it.
/// * [sample_rate] - Sample rate of the input audio in Hz. Must be greater than 1 Hz.
/// * [min_freq] - Minimum frequency to detect in Hz. Must be less than the maximum frequency.
/// * [max_freq] - Maximum frequency to detect in Hz. Must be greater than the minimum frequency.
#[macro_export]
macro_rules! phonk {
    ($batch_size:expr, $sample_rate:expr, $min_freq: expr, $max_freq:expr) => {{
        #[allow(deprecated)]
        $crate::Phonk::<$batch_size, { usize::div_ceil($batch_size, 64) }, { $batch_size / 2 }>::new(
            $sample_rate,
            $min_freq,
            $max_freq,
        )
    }};
}

/// This struct controls an instance of a bitstream autocorrelator which performs monophonic pitch
/// detection. The batch size must be specified at compile time, which makes this `no_std` and
/// `no_alloc` compatible.
///
/// Instances should be created using the [`phonk!`] macro:
///
/// ```no_run
/// use phonk::phonk;
/// let mut phonk = phonk!(4800, 44100, 20, 8000);
/// ```
///
/// This is required because stable Rust lacks the `generic_const_exprs` feature. [`Phonk`]
/// therefore uses additional const generics whose values must satisfy:
///
/// - `WORDS = (N + 63) / 64`
/// - `LAGS = N / 2`
pub struct Phonk<const N: usize, const W: usize, const L: usize> {
    sample_rate: u32,
    min_freq: u32,
    max_freq: u32,

    /// Internal ring buffer of the most recent [N] audio samples.
    data: Ring<f32, N>,

    /// Internal packed zero-crossed bitstream for the current full window.
    bitstream: [u64; W],

    /// Autocorrelation values for lags in the range [min_period, max_period). Since these are
    /// computed using XOR, minimum values represent higher correlation.
    correlations: [u32; L],
}

/// Errors that can occur when constructing a [`Phonk`] instance.
#[derive(Debug)]
pub enum PhonkError {
    /// The batch size `N` must be greater than 1.
    BatchSizeTooSmall,

    /// The batch size `N` must be a multiple of 2.
    BatchSizeNotEven,

    /// The sample rate must be greater than 1 Hz.
    InvalidSampleRate,

    /// The maximum frequency must be greater than the derived minimum frequency.
    MaxFreqNotAboveMinFreq,

    /// The maximum frequency implies a minimum period that does not fit within the batch size.
    MaxFreqPeriodOutOfBounds,
}

/// Intermediary structure describing a recurring period observed in the autocorrelation data.
#[derive(Clone, Copy)]
struct PeriodState {
    /// Last index in correlations where this period was observed.
    index_of_last: usize,

    /// Average magnitude of the observed troughs for this period.
    avg_mag: u32,

    /// Magnitude of the first observed trough for this period, used to determine tolerance.
    first_mag: u32,

    /// Number of troughs observed that are considered equivalent to the first one.
    num_equiv: u8,
}

impl<const N: usize, const W: usize, const L: usize> Phonk<N, W, L> {
    #[doc(hidden)]
    #[deprecated(note = "Construct `Phonk` using the `phonk!` macro instead.")]
    pub const fn new(sample_rate: u32, min_freq: u32, max_freq: u32) -> Result<Self, PhonkError> {
        if N <= 1 {
            return Err(PhonkError::BatchSizeTooSmall);
        }

        if N % 2 != 0 {
            return Err(PhonkError::BatchSizeNotEven);
        }

        if sample_rate <= 1 {
            return Err(PhonkError::InvalidSampleRate);
        }

        if max_freq <= min_freq {
            return Err(PhonkError::MaxFreqNotAboveMinFreq);
        }

        let phonk = Self {
            sample_rate,
            min_freq,
            max_freq,
            data: Ring::new(0.0),
            bitstream: [0u64; W],
            correlations: [0u32; L],
        };

        if phonk.min_period() >= N / 2 {
            return Err(PhonkError::MaxFreqPeriodOutOfBounds);
        }

        if phonk.max_period() <= 1 {
            return Err(PhonkError::MaxFreqPeriodOutOfBounds);
        }

        Ok(phonk)
    }

    /// The minimum period in samples corresponding to the maximum detectable frequency.
    const fn min_period(&self) -> usize {
        self.sample_rate.div_ceil(self.max_freq) as usize
    }

    /// The maximum period in samples corresponding to the minimum detectable frequency.
    const fn max_period(&self) -> usize {
        (self.sample_rate / self.min_freq) as usize
    }

    /// [samples] will be pushed into the internal ring buffer. If there are more samples than the
    /// batch size, only the most recent [N] samples will be saved.
    pub fn push_samples(&mut self, samples: &[f32]) {
        self.data.extend_from_slice(samples);
    }

    /// Run pitch detection on a batch of samples. Running the algorithm before the buffer is
    /// full is undefined behaviour.
    pub fn run(&mut self) -> Option<f64> {
        debug_assert!(self.data.is_full());
        self.zero_cross();
        self.autocorrelate_serial();
        self.subsample_interpolate()
    }

    /// Run pitch detection on a batch of samples, using the provided [executor] to parallelize the
    /// autocorrelation step. Running the algorithm before the buffer is full is undefined behaviour.
    pub fn run_parallel(&mut self, executor: &impl Executor) -> Option<f64> {
        debug_assert!(self.data.is_full());
        self.zero_cross();
        self.autocorrelate_parallel(executor);
        self.subsample_interpolate()
    }

    const HYSTERESIS_THRESHOLD: f32 = 0.01;

    fn zero_cross(&mut self) {
        let mut flag = false;
        let (sample_head, sample_tail) = self.data.as_slices();

        let mut word = 0u64;
        let (mut bit_index, mut word_index) = (0usize, 0usize);

        let mut process_samples = |samples: &[f32]| {
            for &sample in samples {
                flag = if sample >= Self::HYSTERESIS_THRESHOLD {
                    true
                } else if sample <= -Self::HYSTERESIS_THRESHOLD {
                    false
                } else {
                    flag
                };
                word = (word << 1) | flag as u64;

                bit_index += 1;
                if bit_index == 64 {
                    self.bitstream[word_index] = word;
                    word_index += 1;
                    word = 0;
                    bit_index = 0;
                }
            }
        };

        process_samples(sample_head);
        process_samples(sample_tail);

        if bit_index > 0 && word_index < W {
            self.bitstream[word_index] = word << (64 - bit_index);
        }
    }

    /// This function is exposed for debugging purposes.
    #[doc(hidden)]
    pub fn get_correlations(&self) -> &[u32; L] {
        &self.correlations
    }

    fn autocorrelate_serial(&mut self) {
        self.autocorrelate(self.min_period(), self.max_period())
    }

    #[inline(always)]
    fn correlate_lag(bitstream: &[u64; W], lag: usize) -> u32 {
        let word_shift = lag / 64;
        let bit_shift = lag % 64;

        let mut sum = 0u64;
        let limit = W - word_shift - 1;

        for i in 0..limit {
            let a = unsafe { *bitstream.get_unchecked(i) };

            let b = if bit_shift == 0 {
                unsafe { *bitstream.get_unchecked(i + word_shift) }
            } else {
                let b0 = unsafe { *bitstream.get_unchecked(i + word_shift) };
                let b1 = unsafe { *bitstream.get_unchecked(i + word_shift + 1) };
                (b0 << bit_shift) | (b1 >> (64 - bit_shift))
            };

            sum += (a ^ b).count_ones() as u64;
        }

        sum as u32
    }

    fn autocorrelate_parallel(&mut self, executor: &impl Executor) {
        let from = self.min_period();
        let to = self.max_period();
        let bitstream = &self.bitstream;
        let out_addr = self.correlations.as_mut_ptr() as usize;

        // Safety: each task writes only to [from, to) chunk provided by the executor.
        // Correctness requires executor to provide non-overlapping chunks.
        executor.execute(from..to, |from, to| {
            let out_ptr = out_addr as *mut u32;
            for lag in from..to {
                let sum = Self::correlate_lag(bitstream, lag);
                unsafe { *out_ptr.add(lag) = sum };
            }
        });
    }

    fn autocorrelate(&mut self, from: usize, to: usize) {
        for lag in from..to {
            self.correlations[lag] = Self::correlate_lag(&self.bitstream, lag);
        }
    }

    const PERIOD_TOLERANCE: usize = 5;
    const PERIODS_REMOVE: usize = 2;
    const FINISH_THRESHOLD: u8 = 4;

    fn find_lag(&self) -> Option<usize> {
        let (mut peak_count, mut trough_count) = (0usize, 0usize);
        let (mut lowest, mut highest) = (u32::MAX, 0u32);

        let mut active_len = 0usize;
        let mut active = [0usize; L];
        let mut periods: [Option<PeriodState>; L] = [None; L];

        for (index, window) in self.correlations.windows(2).enumerate() {
            debug_assert!(window.len() == 2);
            let (prev, curr) = (window[0], window[1]);
            if peak_count <= trough_count {
                if curr < prev {
                    peak_count += 1;
                    highest = highest.max(prev);
                }
                continue;
            }

            if curr <= prev {
                continue;
            }

            // We have found a trough.
            trough_count += 1;

            let tolerance = (highest.saturating_sub(lowest)) / 5;
            let (mut write, mut matched) = (0usize, false);
            lowest = lowest.min(prev);

            for read in 0..active_len {
                let period = active[read];
                let mut state = periods[period]?;

                if (index - state.index_of_last) / period >= Self::PERIODS_REMOVE {
                    periods[period] = None;
                    continue;
                }

                if !matched
                    && index.abs_diff(state.index_of_last + period) < Self::PERIOD_TOLERANCE
                    && prev.saturating_sub(self.correlations[state.index_of_last]) < tolerance
                {
                    let next_n = state.num_equiv + 1;
                    state = PeriodState {
                        index_of_last: index,
                        avg_mag: (state.num_equiv as u32 * state.avg_mag + prev) / next_n as u32,
                        num_equiv: next_n,
                        first_mag: state.first_mag,
                    };

                    periods[period] = Some(state);
                    matched = true;

                    if next_n >= Self::FINISH_THRESHOLD {
                        return Some(period);
                    }
                }

                active[write] = period;
                write += 1;
            }

            active_len = write;

            if !matched {
                periods[index] = Some(PeriodState {
                    index_of_last: index,
                    avg_mag: prev,
                    num_equiv: 1,
                    first_mag: prev,
                });

                active[active_len] = index;
                active_len += 1;
            }
        }

        let mut min_period = None;

        for &period in &active[..active_len] {
            if min_period.is_none_or(|best| period < best) {
                min_period = Some(period);
            }
        }

        min_period
    }

    fn subsample_interpolate(&self) -> Option<f64> {
        let mut prev = 0.0f32;
        let mut start_edge = 0usize;

        for i in 0..N {
            let sample = self.data[i];
            if sample > 0.0 {
                prev = if i == 0 { 0.0 } else { self.data[i - 1] };
                start_edge = i;
                break;
            }
        }

        let mut dy = self.data[start_edge] - prev;
        let dx1 = -prev / dy;

        let max_index = self.find_lag()?;
        let mut next_edge = max_index - 1;

        while self.data[next_edge] < 0.0 {
            prev = self.data[next_edge];
            next_edge += 1;
        }

        dy = self.data[next_edge] - prev;
        let dx2 = -prev / dy;

        let lag_samples = (next_edge - start_edge) as f32 + (dx2 - dx1);
        let pitch = self.sample_rate as f64 / lag_samples as f64;

        if pitch > self.min_freq as f64 && pitch < self.max_freq as f64 {
            Some(pitch)
        } else {
            None
        }
    }
}
