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

pub fn decode<P: AsRef<Path>>(path: P, extension: &str) -> (Vec<Vec<f32>>, u32, usize) {
    let file = File::open(path).unwrap();
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let mut format = get_probe()
        .format(
            &Hint::new().with_extension(extension),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .unwrap()
        .format;
    let track = format.default_track().unwrap();
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .unwrap();

    let sample_rate = track.codec_params.sample_rate.unwrap();
    let channels = track.codec_params.channels.unwrap().count();

    let mut samples = vec![Vec::<f32>::new(); channels];
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break,
        };
        let decoded = decoder.decode(&packet).unwrap();
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as Duration, *decoded.spec());

        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks_exact(channels) {
            for (channel_index, sample) in frame.iter().enumerate() {
                samples[channel_index].push(*sample);
            }
        }
    }

    (samples, sample_rate, channels)
}
