use clap::{Arg, ArgAction, Command, value_parser};
use owo_colors::OwoColorize;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::{debug, info};
use tracing_subscriber;

mod fonts;
mod overlay;

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

const MM_TO_PT: f64 = 72.0 / 25.4;

#[derive(Clone, Debug)]
struct TrimSize {
    width: f64,
    height: f64,
}

impl FromStr for TrimSize {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (width, height) = match s {
            "trade" => (432.0, 648.0), // 6" × 9"
            "a5" => (420.0, 595.0),
            _ => {
                let (w, h) = s
                    .split_once(|c: char| c == 'x' || c == '×')
                    .ok_or_else(|| format!("invalid size argument: '{}'", s))?;
                let w: f64 = w
                    .trim()
                    .parse()
                    .map_err(|e| format!("invalid width '{}': {}", w, e))?;
                let h: f64 = h
                    .trim()
                    .parse()
                    .map_err(|e| format!("invalid height '{}': {}", h, e))?;
                (w * MM_TO_PT, h * MM_TO_PT)
            }
        };
        Ok(TrimSize { width, height })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the logging subsystem
    tracing_subscriber::fmt::init();

    // Configure command-line argument parser
    let matches = Command::new("cropped")
        .version(VERSION)
        .propagate_version(true)
        .author("Andrew Cowie")
        .about("Place a camera-ready PDF sheet into an A4 page")
        .disable_help_subcommand(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("help")
                .long("help")
                .long_help("Print help")
                .global(true)
                .hide(true)
                .action(ArgAction::Help),
        )
        .arg(
            Arg::new("version")
                .long("version")
                .long_help("Print version")
                .global(true)
                .hide(true)
                .action(ArgAction::Version),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT")
                .value_parser(value_parser!(PathBuf))
                .help("Path for the output PDF.")
                .required(true),
        )
        .arg(
            Arg::new("size")
                .short('s')
                .long("size")
                .value_name("SIZE")
                .help("Trim size of the input manuscript (Either \"trade\", \"a5\", or dimensions in milimetres for example \"140×210\").")
                .value_parser(value_parser!(TrimSize))
                .default_value("trade"),
        )
        .arg(
            Arg::new("manuscript")
                .value_name("INPUT")
                .value_parser(value_parser!(PathBuf))
                .help("Path to the input manuscript PDF to be placed into the template."),
        )
        .get_matches();

    info!("cropped application started");

    //
    // Extract command-line arguments
    //

    let output_path = matches.get_one::<PathBuf>("output").unwrap();

    let manuscript_path = matches.get_one::<PathBuf>("manuscript").unwrap();

    let trim_size = matches.get_one::<TrimSize>("size").unwrap();

    if !manuscript_path.exists() {
        eprintln!("{}: Input manuscript PDF not found.", "error".bright_red());
        std::process::exit(1);
    }

    let (trim_width, trim_height) = (trim_size.width, trim_size.height);

    debug!(?output_path);
    debug!(?manuscript_path);
    debug!(?trim_size);

    // Combine the PDFs
    overlay::combine(output_path, manuscript_path, trim_width, trim_height)?;

    info!("PDF combination completed successfully");

    Ok(())
}
