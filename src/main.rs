use clap::{Arg, ArgAction, Command, value_parser};
use owo_colors::OwoColorize;
use std::path::PathBuf;
use tracing::{debug, info};
use tracing_subscriber;

mod fonts;
mod overlay;

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

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
                .help("Trim size of the input manuscript.")
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

    let trim_size = matches.get_one::<String>("size").unwrap();

    if !manuscript_path.exists() {
        eprintln!(
            "{}: Input manuscript PDF not found.",
            "error".bright_red()
        );
        std::process::exit(1);
    }

    // Parse paper size to dimensions (width, height in points)
    let (trim_width, trim_height) = match trim_size.as_str() {
        "trade" => (432.0, 648.0), // 6" × 9"
        _ => {
            eprintln!(
                "{}: Unknown paper size '{}'. Supported: trade",
                "error".bright_red(),
                trim_size
            );
            std::process::exit(1);
        }
    };

    debug!(?output_path);
    debug!(?manuscript_path);
    debug!(?trim_size);

    // Combine the PDFs
    overlay::combine(output_path, manuscript_path, trim_width, trim_height)?;

    info!("PDF combination completed successfully");

    Ok(())
}
