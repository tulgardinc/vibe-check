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

fn log(min_level: u8, prefix: &str, color_prefix: &str, message: &str) {
    if current_level() < min_level {
        return;
    }
    if use_color() {
        let _ = writeln!(io::stderr(), "{color_prefix}{message}\x1b[0m");
    } else {
        let _ = writeln!(io::stderr(), "{prefix}{message}");
    }
}

pub fn info(message: &str) {
    log(LEVEL_NORMAL, "info  ", "\x1b[36minfo\x1b[0m  ", message);
}

pub fn success(message: &str) {
    log(LEVEL_NORMAL, "done  ", "\x1b[32m\x1b[1mdone\x1b[0m  ", message);
}

pub fn verbose(message: &str) {
    log(LEVEL_VERBOSE, "    ", "\x1b[2m    ", message);
}

pub fn warn(message: &str) {
    log(LEVEL_QUIET, "warn  ", "\x1b[33m\x1b[1mwarn\x1b[0m  ", message);
}

pub fn error(message: &str) {
    log(LEVEL_QUIET, "error ", "\x1b[31m\x1b[1merror\x1b[0m ", message);
}
