use clap::{Arg, ArgAction, Command};
use std::path::Path;
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
                .help("Path for the output PDF.")
                .required(true),
        )
        .arg(
            Arg::new("manuscript")
                .value_name("INPUT")
                .help("Path to the input manuscript PDF to be placed into the template."),
        )
        .get_matches();

    info!("cropped application started");
    debug!("Command-line arguments parsed successfully");

    //
    // Extract command-line arguments
    //

    let template_path = Path::new(match matches.get_one::<String>("template") {
        Some(argument) => argument.as_str(),
        None => TEMPLATE,
    });

    let output_path = Path::new(match matches.get_one::<String>("output") {
        Some(argument) => argument.as_str(),
        None => {
            eprintln!("Output path required");
            std::process::exit(1);
        }
    });

    let manuscript_path = matches.get_one::<String>("manuscript").unwrap();

    // Combine the PDFs
    overlay::combine(
        template_path,
        output_path,
        Path::new(manuscript_path),
    )?;

    info!("PDF combination completed successfully");

    Ok(())
}
