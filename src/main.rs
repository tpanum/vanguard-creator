mod bundle;
mod card;
mod fonts;
mod layout;
mod mse;
mod print_cmd;
mod render;
mod symbols;
mod text;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "vgc",
    about = "Vanguard Card Creator — compose custom MTG Vanguard cards"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render card images from YAML definitions
    Create {
        /// Card YAML files or directories
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Output file or directory (default: <card-name>.png)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Card template image (default: embedded)
        #[arg(long)]
        template: Option<PathBuf>,
    },

    /// Check card definitions without rendering
    Validate {
        /// Card YAML files or directories
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },

    /// Import cards from a Magic Set Editor (.mse-set) file
    #[command(name = "parse-mse")]
    ParseMse {
        /// Path to the .mse-set file
        file: PathBuf,

        /// Output directory for YAML files and artwork
        #[arg(short, long, default_value = ".")]
        output: PathBuf,

        /// Subdirectory name for extracted artwork
        #[arg(long, default_value = "artwork")]
        artwork_dir: String,

        /// Overwrite existing files
        #[arg(long)]
        overwrite: bool,
    },

    /// Arrange card images into a printable PDF
    Print {
        /// Card image files (or use --stdin)
        images: Vec<PathBuf>,

        /// Output PDF path
        #[arg(short, long, default_value = "print.pdf")]
        output: PathBuf,

        /// Page size: a4 or letter
        #[arg(long, default_value = "a4")]
        page_size: String,

        /// Cards per page as COLSxROWS
        #[arg(long, default_value = "3x3")]
        grid: String,

        /// Page margin in millimeters
        #[arg(long, default_value_t = 10.0_f32)]
        margin: f32,

        /// Draw cut lines between cards
        #[arg(long)]
        cut_lines: bool,

        /// Read image paths from stdin (one per line)
        #[arg(long)]
        stdin: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Create {
            paths,
            output,
            template,
        } => render::run(&paths, output.as_deref(), template.as_deref()),
        Commands::Validate { paths } => card::validate_cmd(&paths),
        Commands::ParseMse {
            file,
            output,
            artwork_dir,
            overwrite,
        } => mse::run(&file, &output, &artwork_dir, overwrite),
        Commands::Print {
            images,
            output,
            page_size,
            grid,
            margin,
            cut_lines,
            stdin,
        } => print_cmd::run(images, &output, &page_size, &grid, margin, cut_lines, stdin),
    };

    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
