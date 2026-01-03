use clap::{Arg, ArgAction, Command, value_parser};
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use tracing_subscriber;

mod overlay;

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
                .help(format!("Path to the template PDF with crop marks. The default is {} in the present working directory.", TEMPLATE)),
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
    let template_path = match matches.get_one::<PathBuf>("template") {
        Some(path) => path,
        None => &default,
    };

    let output_path = matches.get_one::<PathBuf>("output").unwrap();

    let manuscript_path = matches.get_one::<PathBuf>("manuscript").unwrap();

    debug!(?template_path);
    debug!(?output_path);
    debug!(?manuscript_path);

    // Combine the PDFs
    overlay::combine(template_path, output_path, manuscript_path)?;

    info!("PDF combination completed successfully");

    Ok(())
}
