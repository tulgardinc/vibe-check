use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicU8, Ordering};

const LEVEL_QUIET: u8 = 0;
const LEVEL_NORMAL: u8 = 1;
const LEVEL_VERBOSE: u8 = 2;

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_NORMAL);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Quiet,
    Normal,
    Verbose,
}

pub fn set_log_level(level: LogLevel) {
    let val = match level {
        LogLevel::Quiet => LEVEL_QUIET,
        LogLevel::Normal => LEVEL_NORMAL,
        LogLevel::Verbose => LEVEL_VERBOSE,
    };
    LOG_LEVEL.store(val, Ordering::Relaxed);
}

fn current_level() -> u8 {
    LOG_LEVEL.load(Ordering::Relaxed)
}

fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err() && io::stderr().is_terminal()
}

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";

pub fn info(message: &str) {
    if current_level() >= LEVEL_NORMAL {
        if use_color() {
            let _ = writeln!(io::stderr(), "{CYAN}info{RESET}  {message}");
        } else {
            let _ = writeln!(io::stderr(), "info  {message}");
        }
    }
}

pub fn success(message: &str) {
    if current_level() >= LEVEL_NORMAL {
        if use_color() {
            let _ = writeln!(io::stderr(), "{GREEN}{BOLD}done{RESET}  {message}");
        } else {
            let _ = writeln!(io::stderr(), "done  {message}");
        }
    }
}

pub fn verbose(message: &str) {
    if current_level() >= LEVEL_VERBOSE {
        if use_color() {
            let _ = writeln!(io::stderr(), "{DIM}    {message}{RESET}");
        } else {
            let _ = writeln!(io::stderr(), "    {message}");
        }
    }
}

pub fn warn(message: &str) {
    if use_color() {
        let _ = writeln!(io::stderr(), "{YELLOW}{BOLD}warn{RESET}  {message}");
    } else {
        let _ = writeln!(io::stderr(), "warn  {message}");
    }
}

pub fn error(message: &str) {
    if use_color() {
        let _ = writeln!(io::stderr(), "{RED}{BOLD}error{RESET} {message}");
    } else {
        let _ = writeln!(io::stderr(), "error {message}");
    }
}
