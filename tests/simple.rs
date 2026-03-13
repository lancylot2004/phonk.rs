use phonk::phonk;
use phonk_helpers::decode;

#[test]
fn can_detect_purish_pitches() {
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
