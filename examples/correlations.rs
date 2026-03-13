use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use phonk::phonk;
use phonk_helpers::decode;
use plotters::prelude::*;

const BATCH: usize = 9_600;

#[derive(Clone, ValueEnum)]
enum ChannelMode {
    /// Use only the first channel.
    First,
    /// Average all channels into mono before processing.
    Average,
}

#[derive(Parser)]
#[command(
    name = "correlations",
    about = "Decode an audio file and either dump or plot per-frame autocorrelation data."
)]
struct Args {
    /// Input audio file.
    input: PathBuf,

    /// Step size between frames in samples.
    #[arg(short, long, default_value_t = 2_400)]
    step_size: usize,

    /// Override sample rate (Hz). If omitted, uses the value from the file.
    #[arg(long)]
    sample_rate: Option<u32>,

    /// Minimum detectable frequency (Hz).
    #[arg(short, long, default_value_t = 20)]
    min_freq: u32,

    /// Maximum detectable frequency (Hz).
    #[arg(short, long, default_value_t = 8_000)]
    max_freq: u32,

    /// How to handle multichannel audio.
    #[arg(short, long, value_enum, default_value_t = ChannelMode::First)]
    channel_mode: ChannelMode,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Dump autocorrelation data to a JSON file.
    Dump {
        /// Output JSON file [default: <input>.json].
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Plot autocorrelation curves to an SVG file.
    Plot {
        /// Output SVG file [default: <input>.svg].
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Only plot the frame at this offset_mult. If omitted, plots all frames.
        #[arg(short, long)]
        frame: Option<usize>,

        /// SVG width in pixels.
        #[arg(long, default_value_t = 1024)]
        width: u32,

        /// SVG height in pixels.
        #[arg(long, default_value_t = 768)]
        height: u32,
    },
}

fn mix_channels(channels: Vec<Vec<f32>>, mode: ChannelMode) -> Vec<f32> {
    match mode {
        ChannelMode::First => channels.into_iter().next().expect("no channels in file"),
        ChannelMode::Average => {
            let n = channels.len() as f32;
            let len = channels[0].len();
            (0..len)
                .map(|i| channels.iter().map(|ch| ch[i]).sum::<f32>() / n)
                .collect()
        }
    }
}

fn collect_correlations(
    samples: &[f32],
    step: usize,
    sample_rate: u32,
    min_freq: u32,
    max_freq: u32,
) -> BTreeMap<usize, Vec<u32>> {
    let mut phonk = phonk!(BATCH, sample_rate, min_freq, max_freq)
        .expect("invalid phonk parameters; check sample rate and max frequency");

    let mut correlations = BTreeMap::new();
    for offset_mult in 0..(samples.len().saturating_sub(BATCH)) / step {
        let offset = offset_mult * step;
        phonk.run(&samples[offset..offset + BATCH]);
        correlations.insert(offset_mult, phonk.get_correlations().to_vec());
    }
    correlations
}

fn cmd_dump(correlations: &BTreeMap<usize, Vec<u32>>, output: PathBuf) {
    std::fs::write(
        &output,
        serde_json::to_string_pretty(correlations).expect("serialisation failed"),
    )
    .expect("could not write output file");
    println!(
        "wrote {} frames to {}",
        correlations.len(),
        output.display()
    );
}

fn cmd_plot(
    correlations: &BTreeMap<usize, Vec<u32>>,
    output: PathBuf,
    frame: Option<usize>,
    width: u32,
    height: u32,
) {
    let entries: Vec<(&usize, &Vec<u32>)> = match frame {
        Some(f) => vec![correlations.get_key_value(&f).unwrap_or_else(|| {
            panic!(
                "frame {f} not found; max is {}",
                correlations.keys().last().unwrap()
            )
        })],
        None => correlations.iter().collect(),
    };

    let lag_count = entries[0].1.len();
    let y_max = entries
        .iter()
        .flat_map(|(_, v)| v.iter())
        .copied()
        .max()
        .unwrap_or(1) as f64;

    let root = SVGBackend::new(&output, (width, height)).into_drawing_area();
    root.fill(&WHITE).unwrap();

    let title = match frame {
        Some(f) => format!("Correlations — frame {f}"),
        None => format!("Correlations — {} frames", entries.len()),
    };

    let mut chart = ChartBuilder::on(&root)
        .caption(&title, ("sans-serif", 5.percent_height()))
        .margin(15)
        .x_label_area_size(8.percent_height())
        .y_label_area_size(10.percent_width())
        .build_cartesian_2d(0usize..lag_count, 0f64..y_max)
        .unwrap();

    chart
        .configure_mesh()
        .x_desc("lag")
        .y_desc("correlation")
        .draw()
        .unwrap();

    let colours: [RGBColor; 7] = [
        RED,
        BLUE,
        GREEN,
        CYAN,
        MAGENTA,
        BLACK,
        RGBColor(255, 165, 0),
    ];

    for (i, (offset_mult, values)) in entries.iter().enumerate() {
        let colour = colours[i % colours.len()];
        chart
            .draw_series(LineSeries::new(
                values.iter().enumerate().map(|(lag, &v)| (lag, v as f64)),
                colour,
            ))
            .unwrap()
            .label(format!("frame {offset_mult}"))
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 10, y)], colour));
    }

    if entries.len() <= 10 {
        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.8))
            .draw()
            .unwrap();
    }

    root.present().unwrap();
}

fn main() {
    let args = Args::parse();

    let (channel_bufs, file_sample_rate) = decode(&args.input);
    let sample_rate = args.sample_rate.unwrap_or(file_sample_rate);
    let samples = mix_channels(channel_bufs, args.channel_mode);

    let correlations = collect_correlations(
        &samples,
        args.step_size,
        sample_rate,
        args.min_freq,
        args.max_freq,
    );

    match args.command {
        Command::Dump { output } => {
            let output = output.unwrap_or_else(|| args.input.with_extension("json"));
            cmd_dump(&correlations, output);
        }
        Command::Plot {
            output,
            frame,
            width,
            height,
        } => {
            let output = output.unwrap_or_else(|| args.input.with_extension("svg"));
            cmd_plot(&correlations, output, frame, width, height);
        }
    }
}
