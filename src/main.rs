use clap::{Parser, Subcommand};
use vibecheck::core::git_pipeline::{run_commit_query, run_git_query, CommitQueryOptions, GitQueryOptions};
use vibecheck::core::index_pipeline::{run_index, IndexOptions, IndexProgress};
use vibecheck::core::query_pipeline::{run_query, QueryOptions};
use vibecheck::core::scan_pipeline::{run_scan, ScanOptions};
use vibecheck::core::status_pipeline::{run_status, StatusOptions};
use vibecheck::ignore::ignore_file::{
    add_exclusion, add_file_exclusion, add_file_pair_exclusion, load_ignore_file, save_ignore_file,
};
use vibecheck::ignore::types::{Exclusion, ExclusionPair, ExclusionSide, FileExclusion, FilePairExclusion};
use vibecheck::output::formatter::{format_human, format_index_json, format_json, format_status_json};
use vibecheck::output::scan_formatter::{format_scan_human, format_scan_json};
use vibecheck::output::types::{DryRunResult, ExcludeResult};
use vibecheck::store::cache::prune_cache;
use vibecheck::util::config::{find_project_root, resolve_cache_path};
use vibecheck::util::git::is_git_repo;
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
        /// Force JSON output
        #[arg(long)]
        json: bool,
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
    Status {
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
    /// Manage exclusions (false positive suppression)
    Exclude {
        #[command(subcommand)]
        action: ExcludeAction,
    },
    /// Check uncommitted changes for similar functions
    Git {
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
    /// Check a specific commit for similar functions
    Commit {
        /// Commit hash to check
        hash: String,
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
    /// Manage the shared embedding cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand)]
enum ExcludeAction {
    /// Exclude a specific function pair from results
    Pair {
        /// Query function name
        #[arg(long)]
        query_function: String,
        /// Query function file path
        #[arg(long)]
        query_path: String,
        /// Query function signature hash
        #[arg(long)]
        query_signature_hash: String,
        /// Candidate function name
        #[arg(long)]
        candidate_function: String,
        /// Candidate function file path
        #[arg(long)]
        candidate_path: String,
        /// Candidate function signature hash
        #[arg(long)]
        candidate_signature_hash: String,
        /// Reason for exclusion
        #[arg(long)]
        reason: String,
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
    /// Exclude files matching a glob pattern from indexing and results
    File {
        /// File path or glob pattern (e.g. "src/generated/**")
        #[arg(long)]
        pattern: String,
        /// Reason for exclusion
        #[arg(long)]
        reason: String,
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
    /// Exclude all comparisons between functions in two files
    FilePair {
        /// First file path
        #[arg(long)]
        file_a: String,
        /// Second file path
        #[arg(long)]
        file_b: String,
        /// Reason for exclusion
        #[arg(long)]
        reason: String,
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
    /// Exclude all pairwise comparisons among a group of files
    FileGroup {
        /// File paths (at least 2)
        #[arg(long, num_args = 2..)]
        files: Vec<String>,
        /// Reason for exclusion
        #[arg(long)]
        reason: String,
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CacheAction {
    /// Remove unreferenced entries from the embedding cache
    Prune {
        /// Force JSON output
        #[arg(long)]
        json: bool,
    },
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
            json,
        } => run_index_cmd(path, db, force, dry_run, json, ollama),
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
        Commands::Status { json } => run_status_cmd(db, json),
        Commands::Exclude { action } => run_exclude_cmd(action),
        Commands::Git {
            top_k,
            threshold,
            json,
        } => run_git_cmd(top_k, threshold, db, json, ollama),
        Commands::Commit {
            hash,
            top_k,
            threshold,
            json,
        } => run_commit_cmd(hash, top_k, threshold, db, json, ollama),
        Commands::Cache { action } => match action {
            CacheAction::Prune { json } => run_cache_prune_cmd(json),
        },
    };

    if let Err(e) = result {
        logger::error(&e.to_string());
        process::exit(1);
    }
}

fn should_use_json(json_flag: bool) -> bool {
    json_flag || !io::stdout().is_terminal()
}

fn run_index_cmd(
    path: Option<String>,
    db: Option<String>,
    force: bool,
    dry_run: bool,
    json: bool,
    ollama: vibecheck::embedder::types::OllamaConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if dry_run {
        let start_dir = path.as_deref().map(Path::new).unwrap_or(Path::new("."));
        let project_root = find_project_root(start_dir);
        let files = vibecheck::util::config::find_source_files(&project_root);
        if should_use_json(json) {
            let result = DryRunResult {
                file_count: files.len(),
                files: files.iter().map(|f| f.display().to_string()).collect(),
            };
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        } else {
            println!("Would index {} source files:", files.len());
            for f in &files {
                println!("  {}", f.display());
            }
        }
        return Ok(());
    }

    struct CliProgress {
        bar: std::sync::Mutex<Option<ProgressBar>>,
        started_at: std::sync::Mutex<Option<std::time::Instant>>,
    }

    impl CliProgress {
        fn format_eta(secs: u64) -> String {
            if secs >= 3600 {
                format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
            } else if secs >= 60 {
                format!("{}m {:02}s", secs / 60, secs % 60)
            } else {
                format!("{}s", secs)
            }
        }
    }

    impl IndexProgress for CliProgress {
        fn on_start(&self, total_batches: usize) {
            let bar = ProgressBar::new(total_batches as u64);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("Embedding [{bar:30}] Batches {pos}/{len} ({elapsed} elapsed{msg})")
                    .unwrap()
                    .progress_chars("=> "),
            );
            bar.enable_steady_tick(std::time::Duration::from_millis(200));
            *self.bar.lock().unwrap_or_else(|e| e.into_inner()) = Some(bar);
            *self.started_at.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(std::time::Instant::now());
        }

        fn on_progress(&self) {
            let guard = self.bar.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref bar) = *guard {
                bar.inc(1);
                let pos = bar.position();
                let total = bar.length().unwrap_or(0);
                if pos > 0 && total > 0 {
                    let elapsed = self
                        .started_at
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .map(|s| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let remaining = elapsed * (total - pos) / pos;
                    bar.set_message(format!(", ~{} remaining", Self::format_eta(remaining)));
                }
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
            started_at: std::sync::Mutex::new(None),
        })),
        cancel: None,
        embedder: None,
    })?;

    if should_use_json(json) {
        println!("{}", format_index_json(&result));
    } else {
        logger::success(&format!(
            "Indexed {} functions from {} files. Model: {} ({}, {}d).",
            result.functions_indexed,
            result.files_scanned,
            result.model,
            result.tier,
            result.dimensions
        ));
    }

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

    if should_use_json(json) {
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

    if should_use_json(json) {
        println!("{}", format_scan_json(&result));
    } else {
        let project_root = find_project_root(Path::new("."));
        print!("{}", format_scan_human(&result, &project_root));
    }

    Ok(())
}

fn run_status_cmd(db: Option<String>, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let result = run_status(StatusOptions { db_path: db })?;
    if should_use_json(json) {
        println!("{}", format_status_json(&result));
    } else {
        println!("{}", vibecheck::output::formatter::format_status_human(&result));
    }
    Ok(())
}

fn run_exclude_cmd(action: ExcludeAction) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = find_project_root(Path::new("."));
    let ignore_file = load_ignore_file(&project_root);
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let (message, updated, json) = match action {
        ExcludeAction::Pair {
            query_function,
            query_path,
            query_signature_hash,
            candidate_function,
            candidate_path,
            candidate_signature_hash,
            reason,
            json,
        } => {
            let updated = add_exclusion(
                &ignore_file,
                Exclusion {
                    reason: reason.clone(),
                    added: today,
                    pair: ExclusionPair {
                        a: ExclusionSide {
                            path: query_path,
                            function: query_function.clone(),
                            signature_hash: query_signature_hash,
                        },
                        b: ExclusionSide {
                            path: candidate_path,
                            function: candidate_function.clone(),
                            signature_hash: candidate_signature_hash,
                        },
                    },
                },
            );
            let msg = format!(
                "Exclusion added: {} \u{2194} {} (\"{}\").",
                query_function, candidate_function, reason
            );
            (msg, updated, json)
        }
        ExcludeAction::File {
            pattern,
            reason,
            json,
        } => {
            let updated = add_file_exclusion(
                &ignore_file,
                FileExclusion {
                    pattern: pattern.clone(),
                    reason: reason.clone(),
                    added: today,
                },
            );
            let msg = format!("File exclusion added: \"{}\" (\"{}\").", pattern, reason);
            (msg, updated, json)
        }
        ExcludeAction::FilePair {
            file_a,
            file_b,
            reason,
            json,
        } => {
            let updated = add_file_pair_exclusion(
                &ignore_file,
                FilePairExclusion {
                    a: file_a.clone(),
                    b: file_b.clone(),
                    reason: reason.clone(),
                    added: today,
                },
            );
            let msg = format!(
                "File pair exclusion added: {} \u{2194} {} (\"{}\").",
                file_a, file_b, reason
            );
            (msg, updated, json)
        }
        ExcludeAction::FileGroup {
            files,
            reason,
            json,
        } => {
            if files.len() < 2 {
                return Err("Need at least 2 files to create group exclusions".into());
            }
            let mut current = ignore_file.clone();
            let mut added = 0;
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let before = current.file_pair_exclusions.len();
                    current = add_file_pair_exclusion(
                        &current,
                        FilePairExclusion {
                            a: files[i].clone(),
                            b: files[j].clone(),
                            reason: reason.clone(),
                            added: today.clone(),
                        },
                    );
                    if current.file_pair_exclusions.len() > before {
                        added += 1;
                    }
                }
            }
            let total_pairs = files.len() * (files.len() - 1) / 2;
            let msg = format!(
                "Group exclusion: {} files, {} pairs added ({} already existed).",
                files.len(),
                added,
                total_pairs - added
            );
            (msg, current, json)
        }
    };

    save_ignore_file(&project_root, &updated);

    let result = ExcludeResult {
        message,
        total_exclusions: updated.exclusions.len(),
        total_file_exclusions: updated.file_exclusions.len(),
        total_file_pair_exclusions: updated.file_pair_exclusions.len(),
    };

    if should_use_json(json) {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        println!("{}", result.message);
    }

    Ok(())
}

fn run_git_cmd(
    top_k: usize,
    threshold: f64,
    db: Option<String>,
    json: bool,
    ollama: vibecheck::embedder::types::OllamaConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = run_git_query(GitQueryOptions {
        top_k,
        threshold,
        db_path: db,
        project_root: None,
        ollama,
        embedder: None,
    })?;

    if should_use_json(json) {
        println!("{}", format_json(&result));
    } else {
        print!("{}", format_human(&result));
    }

    Ok(())
}

fn run_commit_cmd(
    hash: String,
    top_k: usize,
    threshold: f64,
    db: Option<String>,
    json: bool,
    ollama: vibecheck::embedder::types::OllamaConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = run_commit_query(CommitQueryOptions {
        hash,
        top_k,
        threshold,
        db_path: db,
        project_root: None,
        ollama,
        embedder: None,
    })?;

    if should_use_json(json) {
        println!("{}", format_json(&result));
    } else {
        print!("{}", format_human(&result));
    }

    Ok(())
}

fn run_cache_prune_cmd(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = find_project_root(Path::new("."));

    if !is_git_repo(&project_root) {
        return Err("Not a git repository. Cache prune requires git.".into());
    }

    let cache_path = resolve_cache_path(&project_root)
        .ok_or("Could not resolve cache path. Is this a git repository?")?;

    let result = prune_cache(&cache_path, &project_root)?;

    if should_use_json(json) {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        println!(
            "Pruned {} entries, freed {} bytes.",
            result.entries_removed, result.bytes_freed
        );
    }

    Ok(())
}
