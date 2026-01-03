use clap::{Arg, ArgAction, Command, value_parser};
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use tracing_subscriber;

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const TEMPLATE: &str = "CropMarks_A4.pdf";

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
            Arg::new("template")
                .short('t')
                .long("template")
                .value_name("TEMPLATE")
                .help("Path to the template PDF with crop marks."),
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

    let default = PathBuf::from(TEMPLATE);
    let template = match matches.get_one::<PathBuf>("template") {
        Some(path) => path,
        None => &default,
    };

    let output = matches.get_one::<PathBuf>("output").unwrap();

    let manuscript = matches.get_one::<PathBuf>("manuscript").unwrap();

    debug!(?template);
    debug!(?output);
    debug!(?manuscript);

    // Combine the PDFs
    overlay::combine(template, output, manuscript)?;

    info!("PDF combination completed successfully");

    Ok(())
}
