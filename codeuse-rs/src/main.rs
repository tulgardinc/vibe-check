use clap::{Parser, Subcommand};
use vibecheck::core::index_pipeline::{run_index, IndexOptions};
use vibecheck::core::query_pipeline::{run_query, QueryOptions};
use vibecheck::core::scan_pipeline::{run_scan, ScanOptions};
use vibecheck::core::status_pipeline::{run_status, StatusOptions};
use vibecheck::output::formatter::{format_human, format_json};
use vibecheck::output::scan_formatter::{format_scan_human, format_scan_json};
use vibecheck::util::config::find_project_root;
use vibecheck::util::logger::{self, LogLevel};
use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::process;

#[derive(Parser)]
#[command(name = "vibec", version = "0.1.0", about = "Semantic code reuse enforcement")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index TypeScript functions in the codebase
    Index {
        /// Directory to index
        path: Option<String>,
        /// Database file path
        #[arg(long)]
        db: Option<String>,
        /// Force full re-index
        #[arg(long)]
        force: bool,
        /// Verbose output
        #[arg(long)]
        verbose: bool,
        /// Show what would be indexed without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Find existing functions similar to new code
    Query {
        /// TypeScript file to check
        file: String,
        /// Read from stdin instead of file
        #[arg(long)]
        stdin: bool,
        /// Number of candidates per function
        #[arg(long, default_value = "5")]
        top_k: usize,
        /// Cosine distance threshold
        #[arg(long, default_value = "0.3")]
        threshold: f64,
        /// Database file path
        #[arg(long)]
        db: Option<String>,
        /// Force JSON output
        #[arg(long)]
        json: bool,
        /// Verbose output
        #[arg(long)]
        verbose: bool,
    },
    /// Scan the entire indexed codebase for similar function pairs
    Scan {
        /// Maximum pairs to show
        #[arg(long, default_value = "50")]
        top_n: usize,
        /// Cosine distance threshold
        #[arg(long, default_value = "0.25")]
        threshold: f64,
        /// Database file path
        #[arg(long)]
        db: Option<String>,
        /// Force JSON output
        #[arg(long)]
        json: bool,
        /// Verbose output
        #[arg(long)]
        verbose: bool,
    },
    /// Show index health and statistics
    Status {
        /// Database file path
        #[arg(long)]
        db: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Index {
            path,
            db,
            force,
            verbose,
            dry_run,
        } => run_index_cmd(path, db, force, verbose, dry_run),
        Commands::Query {
            file,
            stdin,
            top_k,
            threshold,
            db,
            json,
            verbose,
        } => run_query_cmd(file, stdin, top_k, threshold, db, json, verbose),
        Commands::Scan {
            top_n,
            threshold,
            db,
            json,
            verbose,
        } => run_scan_cmd(top_n, threshold, db, json, verbose),
        Commands::Status { db } => run_status_cmd(db),
    };

    if let Err(e) = result {
        logger::error(&e.to_string());
        process::exit(1);
    }
}

fn run_index_cmd(
    path: Option<String>,
    db: Option<String>,
    force: bool,
    verbose: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        logger::set_log_level(LogLevel::Verbose);
    }

    if dry_run {
        let start_dir = path.as_deref().map(Path::new).unwrap_or(Path::new("."));
        let project_root = find_project_root(start_dir);
        let files = vibecheck::util::config::find_typescript_files(&project_root);
        println!("Would index {} TypeScript files:", files.len());
        for f in &files {
            println!("  {}", f.display());
        }
        return Ok(());
    }

    let result = run_index(IndexOptions {
        path,
        db_path: db,
        force,
        on_progress: Some(Box::new(|msg| {
            logger::verbose(msg);
        })),
    })?;

    logger::success(&format!(
        "Indexed {} functions from {} files. Model: {} ({}, {}d).",
        result.functions_indexed,
        result.files_scanned,
        result.model,
        result.tier,
        result.dimensions
    ));

    Ok(())
}

fn run_query_cmd(
    file: String,
    stdin: bool,
    top_k: usize,
    threshold: f64,
    db: Option<String>,
    json: bool,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        logger::set_log_level(LogLevel::Verbose);
    }

    let source = if stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(&file)?
    };

    let result = run_query(QueryOptions {
        source,
        file_name: Some(file),
        top_k,
        threshold,
        db_path: db,
        project_root: None,
    })?;

    let use_json = json || !io::stdout().is_terminal();
    if use_json {
        println!("{}", format_json(&result));
    } else {
        print!("{}", format_human(&result));
    }

    Ok(())
}

fn run_scan_cmd(
    top_n: usize,
    threshold: f64,
    db: Option<String>,
    json: bool,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        logger::set_log_level(LogLevel::Verbose);
    }

    let result = run_scan(ScanOptions {
        top_n,
        threshold,
        db_path: db,
        project_root: None,
        on_progress: Some(Box::new(|msg| {
            logger::verbose(msg);
        })),
    })?;

    let use_json = json || !io::stdout().is_terminal();
    if use_json {
        println!("{}", format_scan_json(&result));
    } else {
        let project_root = find_project_root(Path::new("."));
        print!("{}", format_scan_human(&result, &project_root));
    }

    Ok(())
}

fn run_status_cmd(db: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let result = run_status(StatusOptions { db_path: db })?;

    if !result.exists {
        println!("No index found. Run `vibec index` to create one.");
        return Ok(());
    }

    println!("Database: {} ({} MB)", result.db_path, result.size_mb);
    println!("Model: {}", result.model);
    println!("Dimensions: {}", result.dimensions);

    let unembedded_suffix = if result.unembedded > 0 {
        format!(" ({} awaiting embedding)", result.unembedded)
    } else {
        String::new()
    };
    println!(
        "Indexed functions: {}{}",
        result.indexed_functions, unembedded_suffix
    );
    println!("Tracked files: {}", result.tracked_files);
    println!("Last indexed: {}", result.last_indexed);

    let stale_suffix = if result.stale_exclusions > 0 {
        format!(" ({} stale)", result.stale_exclusions)
    } else {
        String::new()
    };
    println!("Exclusions: {}{}", result.exclusions, stale_suffix);

    Ok(())
}
