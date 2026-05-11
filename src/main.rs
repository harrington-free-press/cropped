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

/// Parse "WxH" or "W×H" with values in millimetres, returning points.
fn parse_mm_dimensions(s: &str) -> Result<(f64, f64), String> {
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
    Ok((w * MM_TO_PT, h * MM_TO_PT))
}

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
            _ => parse_mm_dimensions(s)?,
        };
        Ok(TrimSize { width, height })
    }
}

#[derive(Clone, Debug)]
struct PaperSize {
    width: f64,
    height: f64,
}

impl FromStr for PaperSize {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (width, height) = match s {
            "a4" => (595.0, 842.0),       // 210 × 297 mm
            "a3" => (842.0, 1191.0),      // 297 × 420 mm
            "a2" => (1191.0, 1684.0),     // 420 × 594 mm
            "letter" => (612.0, 792.0),   // 8.5 × 11 in
            _ => parse_mm_dimensions(s)?,
        };
        Ok(PaperSize { width, height })
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
        .about("Place a camera-ready PDF sheet into a larger page with crop marks")
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
                .help("Trim size of the input manuscript. Either \"a5\", \"trade\", or dimensions in millimetres (for example \"140x210\"). Note that this is the size the crop marks will be set at; the input document may be somewhat larger if it has bleed.")
                .value_parser(value_parser!(TrimSize))
                .default_value("trade"),
        )
        .arg(
            Arg::new("paper")
                .short('p')
                .long("paper")
                .value_name("PAPER")
                .help("Paper size the manuscript will be placed onto. One of \"a2\", \"a3\", \"a4\", \"letter\", or you can specify the dimensions in millimetres.")
                .value_parser(value_parser!(PaperSize))
                .default_value("a4"),
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

    let paper_size = matches.get_one::<PaperSize>("paper").unwrap();

    if !manuscript_path.exists() {
        eprintln!("{}: Input manuscript PDF not found.", "error".bright_red());
        std::process::exit(1);
    }

    debug!(?output_path);
    debug!(?manuscript_path);
    debug!(?trim_size);
    debug!(?paper_size);

    // Combine the PDFs
    overlay::combine(
        output_path,
        manuscript_path,
        trim_size.width,
        trim_size.height,
        paper_size.width,
        paper_size.height,
    )?;

    info!("PDF combination completed successfully");

    Ok(())
}
