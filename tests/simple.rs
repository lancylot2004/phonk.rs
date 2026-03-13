use phonk::phonk;
use phonk_helpers::decode;

#[test]
fn can_detect_purish_pitches() {
    let (samples_by_channel, sample_rate) = decode("tests/assets/fork-440.mp3");
    let samples = samples_by_channel
        .first()
        .expect("decoded audio has no channels");
    let mut phonk = phonk!(9_600, sample_rate, 20, 8_000).unwrap();

    for offset_mult in 0..(samples.len() - 9_600) / 2_400 {
        let offset = offset_mult * 2_400;
        let pitch = phonk.run(&samples[offset..offset + 9_600]);
        let pitch_string = match pitch {
            Some((max_index, pitch)) => {
                let naive_pitch = max_index as f64 * sample_rate as f64 / 9600.0;
                format!(
                    "Pitch: {pitch:.2} Hz, Index: {max_index}, Naive Pitch: {naive_pitch:.2} Hz"
                )
            }
            None => "None".to_string(),
        };
        println!("Offset: {offset:>6}, {pitch_string}");
    }
}
