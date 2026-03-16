use clap::{Parser, Subcommand};
use vibecheck::core::index_pipeline::{run_index, IndexOptions};
use vibecheck::core::query_pipeline::{run_query, QueryOptions};
use vibecheck::core::scan_pipeline::{run_scan, ScanOptions};
use vibecheck::core::status_pipeline::{run_status, StatusOptions};
use vibecheck::output::formatter::{format_human, format_json};
use vibecheck::output::scan_formatter::{format_scan_human, format_scan_json};
use vibecheck::util::config::find_project_root;
use vibecheck::util::logger::{self, LogLevel};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::process;

#[derive(Parser)]
#[command(name = "vibec", version = "0.1.0", about = "Semantic code reuse enforcement")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Ollama embedding model name (overrides VIBECHECK_MODEL env var)
    #[arg(long, global = true)]
    model: Option<String>,

    /// Ollama server URL (overrides OLLAMA_HOST env var)
    #[arg(long, global = true)]
    ollama_host: Option<String>,
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

    let model = cli.model;
    let ollama_host = cli.ollama_host;

    let result = match cli.command {
        Commands::Index {
            path,
            db,
            force,
            verbose,
            dry_run,
        } => run_index_cmd(path, db, force, verbose, dry_run, model, ollama_host),
        Commands::Query {
            file,
            stdin,
            top_k,
            threshold,
            db,
            json,
            verbose,
        } => run_query_cmd(file, stdin, top_k, threshold, db, json, verbose, model, ollama_host),
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
    model: Option<String>,
    ollama_host: Option<String>,
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

    use std::sync::Arc;

    let pb: Arc<std::sync::Mutex<Option<ProgressBar>>> = Arc::new(std::sync::Mutex::new(None));

    let pb_start = Arc::clone(&pb);
    let pb_progress = Arc::clone(&pb);
    let pb_done = Arc::clone(&pb);

    let result = run_index(IndexOptions {
        path,
        db_path: db,
        force,
        model,
        ollama_host,
        on_embed_start: Some(Box::new(move |total| {
            let bar = ProgressBar::new(total as u64);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("{msg} [{bar:30}] {pos}/{len} ({eta})")
                    .unwrap()
                    .progress_chars("=> "),
            );
            bar.set_message("Embedding");
            *pb_start.lock().unwrap() = Some(bar);
        })),
        on_embed_progress: Some(Box::new(move |n| {
            if let Some(ref bar) = *pb_progress.lock().unwrap() {
                bar.inc(n as u64);
            }
        })),
        on_embed_done: Some(Box::new(move || {
            if let Some(ref bar) = *pb_done.lock().unwrap() {
                bar.finish_and_clear();
            }
        })),
        cancel: None,
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
    model: Option<String>,
    ollama_host: Option<String>,
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
        model,
        ollama_host,
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
