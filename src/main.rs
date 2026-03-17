use clap::{Parser, Subcommand};
use vibecheck::core::index_pipeline::{run_index, IndexOptions, IndexProgress};
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

    /// Override model context length in tokens (derives max input bytes)
    #[arg(long, global = true)]
    context_length: Option<usize>,

    /// Override max input bytes for truncation
    #[arg(long, global = true)]
    max_input_bytes: Option<usize>,

    /// Query prefix prepended to search inputs (overrides VIBECHECK_QUERY_PREFIX env var; auto-detected for Nomic models)
    #[arg(long, global = true)]
    query_prefix: Option<String>,

    /// Override embedding dimensions (overrides VIBECHECK_DIMENSIONS env var)
    #[arg(long, global = true)]
    dimensions: Option<usize>,

    /// Database file path
    #[arg(long, global = true)]
    db: Option<String>,

    /// Verbose output
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Index functions in the codebase
    Index {
        /// Directory to index
        path: Option<String>,
        /// Force full re-index
        #[arg(long)]
        force: bool,
        /// Show what would be indexed without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Find existing functions similar to new code
    Query {
        /// Source file to check
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
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
    /// Scan the entire indexed codebase for similar function pairs
    Scan {
        /// Maximum pairs to show
        #[arg(long, default_value = "50")]
        top_n: usize,
        /// Cosine distance threshold
        #[arg(long, default_value = "0.25")]
        threshold: f64,
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show index health and statistics
    Status,
}

fn main() {
    let cli = Cli::parse();

    if cli.verbose {
        logger::set_log_level(LogLevel::Verbose);
    }

    let ollama = vibecheck::embedder::types::OllamaConfig {
        model: cli.model,
        host: cli.ollama_host,
        context_length: cli.context_length,
        max_input_bytes: cli.max_input_bytes,
        query_prefix: cli.query_prefix,
        dimensions: cli.dimensions,
    };
    let db = cli.db;

    let result = match cli.command {
        Commands::Index {
            path,
            force,
            dry_run,
        } => run_index_cmd(path, db, force, dry_run, ollama),
        Commands::Query {
            file,
            stdin,
            top_k,
            threshold,
            json,
        } => run_query_cmd(file, stdin, top_k, threshold, db, json, ollama),
        Commands::Scan {
            top_n,
            threshold,
            json,
        } => run_scan_cmd(top_n, threshold, db, json),
        Commands::Status => run_status_cmd(db),
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
    dry_run: bool,
    ollama: vibecheck::embedder::types::OllamaConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if dry_run {
        let start_dir = path.as_deref().map(Path::new).unwrap_or(Path::new("."));
        let project_root = find_project_root(start_dir);
        let files = vibecheck::util::config::find_source_files(&project_root);
        println!("Would index {} source files:", files.len());
        for f in &files {
            println!("  {}", f.display());
        }
        return Ok(());
    }

    struct CliProgress {
        bar: std::sync::Mutex<Option<ProgressBar>>,
    }

    impl IndexProgress for CliProgress {
        fn on_start(&self, total: usize) {
            let bar = ProgressBar::new(total as u64);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("{msg} [{bar:30}] {pos}/{len} ({elapsed} elapsed, {eta} remaining)")
                    .unwrap()
                    .progress_chars("=> "),
            );
            bar.set_message("Embedding");
            bar.enable_steady_tick(std::time::Duration::from_millis(200));
            *self.bar.lock().unwrap_or_else(|e| e.into_inner()) = Some(bar);
        }

        fn on_progress(&self, count: usize) {
            if let Some(ref bar) = *self.bar.lock().unwrap_or_else(|e| e.into_inner()) {
                bar.inc(count as u64);
            }
        }

        fn on_done(&self) {
            if let Some(ref bar) = *self.bar.lock().unwrap_or_else(|e| e.into_inner()) {
                bar.finish_and_clear();
            }
        }
    }

    let result = run_index(IndexOptions {
        path,
        db_path: db,
        force,
        ollama,
        progress: Some(Box::new(CliProgress {
            bar: std::sync::Mutex::new(None),
        })),
        cancel: None,
        embedder: None,
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
    ollama: vibecheck::embedder::types::OllamaConfig,
) -> Result<(), Box<dyn std::error::Error>> {
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
        ollama,
        embedder: None,
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
) -> Result<(), Box<dyn std::error::Error>> {
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
    println!("{}", vibecheck::output::formatter::format_status_human(&result));
    Ok(())
}
