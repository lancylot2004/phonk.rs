use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Duration;
use symphonia::default::{get_codecs, get_probe};

/// Reads an audio file into its channels of raw data and sample rate.
pub fn decode<P: AsRef<Path>>(path: P) -> (Vec<Vec<f32>>, u32) {
    let ext = path
        .as_ref()
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_owned();

    let file = File::open(&path).expect("could not open audio file");
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let mut format = get_probe()
        .format(
            &Hint::new().with_extension(&ext),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .expect("unsupported format")
        .format;

    let track = format.default_track().expect("no default track");
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .expect("unsupported codec");

    let sample_rate = track.codec_params.sample_rate.expect("unknown sample rate");
    let channels = track
        .codec_params
        .channels
        .expect("unknown channel layout")
        .count();

    let mut channel_bufs = vec![Vec::<f32>::new(); channels];

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        let decoded = decoder.decode(&packet).expect("decode error");
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as Duration, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks_exact(channels) {
            for (ch, &s) in frame.iter().enumerate() {
                channel_bufs[ch].push(s);
            }
        }
    }

    (channel_bufs, sample_rate)
}
