#![no_std]

/// Constructs a [Phonk] with correctly derived const parameters (`W = (N + 63) / 64`,
/// `L = N / 2`), where `N`, `W`, and `L` are the batch size, number of 64-bit words needed for the
/// given batch size, and number of lags to compute, respectively. The macro also performs
/// compile-time validation of the input parameters.
///
/// * [batch_size] - Number of samples to process in each batch. Must be a multiple of 2 and greater
/// than 1. Scaling this parameter will impact performance linearly, and the minimum detectable
/// frequency is inversely proportional to it.
/// * [sample_rate] - Sample rate of the input audio in Hz. Must be greater than 1 Hz.
/// * [max_freq] - Maximum frequency to detect in Hz. Must be greater than the minimum frequency
/// implied by [batch_size].
#[macro_export]
macro_rules! phonk {
    ($batch_size:expr, $sample_rate:expr, $max_freq:expr) => {{
        #[allow(deprecated)]
        $crate::Phonk::<$batch_size, { usize::div_ceil($batch_size, 64) }, { $batch_size / 2 }>::new(
            $sample_rate,
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
/// let mut phonk = phonk!(4800, 44100, 8000f64);
/// ```
///
/// This is required because stable Rust lacks the `generic_const_exprs` feature. [`Phonk`]
/// therefore uses additional const generics whose values must satisfy:
///
/// - `WORDS = (N + 63) / 64`
/// - `LAGS = N / 2`
pub struct Phonk<const N: usize, const W: usize, const L: usize> {
    sample_rate: u32,
    min_period: u32,
    min_freq: f64,
    max_freq: f64,

    bitstream: [u64; W],
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

/// The minimum detectable frequency derived from the sample rate and batch size.
pub const fn min_freq(sample_rate: u32, batch_size: usize) -> f64 {
    sample_rate as f64 / (batch_size as f64 / 2.0)
}

impl<const N: usize, const W: usize, const L: usize> Phonk<N, W, L> {
    #[doc(hidden)]
    #[deprecated(note = "Construct `Phonk` using the `phonk!` macro instead.")]
    pub const fn new(sample_rate: u32, max_freq: f64) -> Result<Self, PhonkError> {
        if N <= 1 {
            return Err(PhonkError::BatchSizeTooSmall);
        }

        if N % 2 != 0 {
            return Err(PhonkError::BatchSizeNotEven);
        }

        if sample_rate <= 1 {
            return Err(PhonkError::InvalidSampleRate);
        }

        let min_freq = min_freq(sample_rate, N);
        if max_freq <= min_freq {
            return Err(PhonkError::MaxFreqNotAboveMinFreq);
        }

        let min_period = sample_rate.div_ceil(N as u32 / 2);
        if min_period >= N as u32 {
            return Err(PhonkError::MaxFreqPeriodOutOfBounds);
        }

        Ok(Self {
            sample_rate,
            min_period,
            min_freq,
            max_freq,
            bitstream: [0u64; W],
            correlations: [0u32; L],
        })
    }

    /// Run pitch detection on a batch of samples. This will not trigger the callback.
    pub fn run(&mut self, samples: &[f32]) -> Option<f64> {
        self.zero_cross(samples);
        self.autocorrelate();
        self.estimate(samples)
    }

    fn zero_cross(&mut self, samples: &[f32]) {
        debug_assert!(samples.len() == N);

        let mut word = 0u64;
        let (mut bit_index, mut word_index) = (0usize, 0usize);

        for &sample in samples {
            word <<= 1;
            word |= (sample >= 0.0) as u64;

            bit_index += 1;
            if bit_index == 64 {
                self.bitstream[word_index] = word;
                word_index += 1;
                bit_index = 0;
                word = 0;
            }
        }
    }

    fn autocorrelate(&mut self) {
        let words = (N + 63) / 64;

        for lag in 0..(N / 2) {
            let word_shift = lag / 64;
            let bit_shift = lag % 64;

            let mut sum = 0u32;
            let limit = words.saturating_sub(word_shift + 1);

            for i in 0..limit {
                let a = self.bitstream[i];
                let b = match bit_shift {
                    0 => self.bitstream[i + word_shift],
                    shift => {
                        (self.bitstream[i + word_shift] << shift)
                            | (self.bitstream[i + word_shift + 1] >> (64 - shift))
                    }
                };

                sum += (a ^ b).count_ones();
            }

            self.correlations[lag] = sum;
        }
    }

    fn estimate(&self, samples: &[f32]) -> Option<f64> {
        // strongest correlation (remember XOR correlation is inverted)
        let min_corr = *self.correlations.iter().max()? as i32;

        let start = self.min_period as usize;

        let (max_idx_rel, _) = self.correlations[start..]
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| *v)?;

        let mut max_index = max_idx_rel + start;

        let harmonic_threshold = 0.15 * min_corr as f64;
        let max_division = max_index / start;

        for division in (1..=max_division).rev() {
            let mut strong = true;

            for i in 1..division {
                let idx = (i * max_index) / division;

                if (self.correlations[idx] as f64) > harmonic_threshold {
                    strong = false;
                    break;
                }
            }

            if strong {
                max_index /= division;
                break;
            }
        }

        // first zero crossing
        let mut prev = 0.0f32;
        let mut start_edge = 0usize;

        for (i, &sample) in samples.iter().enumerate() {
            if sample > 0.0 {
                prev = if i == 0 { 0.0 } else { samples[i - 1] };
                start_edge = i;
                break;
            }
        }

        let mut dy = samples[start_edge] - prev;
        let dx1 = -prev / dy;

        // second zero crossing near peak
        let mut next_edge = max_index - 1;

        while samples[next_edge] < 0.0 {
            prev = samples[next_edge];
            next_edge += 1;
        }

        dy = samples[next_edge] - prev;
        let dx2 = -prev / dy;

        let lag_samples = (next_edge - start_edge) as f32 + (dx2 - dx1);

        let pitch = self.sample_rate as f64 / lag_samples as f64;

        if pitch > self.min_freq && pitch < self.max_freq {
            Some(pitch)
        } else {
            None
        }
    }
}
