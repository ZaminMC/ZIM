//! Log pipeline primitives: line parsing and the bounded ring.
//!
//! Parser input is the server's stdout — Minecraft/Paper-family format
//! `[HH:MM:SS] [Thread/LEVEL]: message`, ANSI-colored in some setups. The
//! parser is load-bearing: the state machine, metrics, and crash reports
//! all read from it (see TESTING.md for golden fixtures).

use zamin_protocol::streams::{LogLevel, LogLine};

/// Strip ANSI SGR (color) sequences. Length is preserved conceptually — we
/// drop only escape bytes, never message content.
pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // Skip until a final byte in @-~ (CSI) or the whole sequence.
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC sequence: skip until BEL or ST.
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if c == '\u{7}' || (prev == '\u{1b}' && c == '\\') {
                        break;
                    }
                    prev = c;
                }
            }
            Some(other) if ('@'..='~').contains(&other) => {}
            Some(_) => {}
            None => break,
        }
    }
    out
}

/// Parse one raw line into a [`LogLine`]. `ts_ms` is ingestion time — the
/// JVM's local-time clock string is displayed raw by the log viewer and is
/// never converted into false precision (ADR-0007, protocol spec §9).
pub fn parse_line(raw: &str, ts_ms: i64) -> LogLine {
    let line = strip_ansi(raw);
    let line = line.trim_end_matches(['\r', '\n']);
    let (thread, level, message) = parse_structured(line);
    LogLine {
        ts_ms,
        level,
        thread,
        line: message.to_owned(),
    }
}

/// `(thread, level, message)` when the line matches the structured format;
/// otherwise the whole line is the message with [`LogLevel::Unknown`].
fn parse_structured(line: &str) -> (Option<String>, LogLevel, &str) {
    // Shape: [12:34:56] [Thread name/LEVEL]: message
    let Some((first, rest)) = line.split_once("] [") else {
        return (None, LogLevel::Unknown, line);
    };
    if !first.starts_with('[') || first.len() < 2 {
        return (None, LogLevel::Unknown, line);
    }
    let Some((thread, level_and_msg)) = rest.split_once("]: ") else {
        return (None, LogLevel::Unknown, line);
    };
    let Some((thread, level)) = thread.rsplit_once('/') else {
        return (None, LogLevel::Unknown, line);
    };
    (Some(thread.to_owned()), parse_level(level), level_and_msg)
}

fn parse_level(level: &str) -> LogLevel {
    match level.trim().to_ascii_uppercase().as_str() {
        "INFO" => LogLevel::Info,
        "WARN" | "WARNING" => LogLevel::Warn,
        "ERROR" | "FATAL" | "SEVERE" => LogLevel::Error,
        "DEBUG" | "TRACE" => LogLevel::Debug,
        _ => LogLevel::Unknown,
    }
}

/// True when the line is the Paper-family startup completion signature —
/// the startup-validation signal (ADR-0005). Tolerant of extra text the
/// fork might append.
pub fn is_startup_complete(line: &str) -> bool {
    let lower = strip_ansi(line).to_ascii_lowercase();
    lower.contains("done (") && lower.contains("! for help")
}

/// True when the line indicates the server is stopping by itself.
pub fn is_stopping(line: &str) -> bool {
    let lower = strip_ansi(line).to_ascii_lowercase();
    lower.contains("stopping server") || lower.contains("stopping the server")
}

/// Bounded ring: the daemon's in-memory backscroll, bounded by *servers*,
/// never by clients (ADR-0006).
#[derive(Debug)]
pub struct Ring<T> {
    items: std::collections::VecDeque<T>,
    capacity: usize,
    total_pushed: u64,
}

impl<T> Ring<T> {
    pub fn new(capacity: usize) -> Self {
        Ring {
            items: std::collections::VecDeque::with_capacity(capacity.min(1024)),
            capacity: capacity.max(1),
            total_pushed: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
        self.total_pushed += 1;
    }

    pub fn total_pushed(&self) -> u64 {
        self.total_pushed
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_paper_lines() {
        let line = parse_line(
            "[05:24:11] [Server thread/INFO]: Done (3.214s)! For help, type \"help\"",
            1000,
        );
        assert_eq!(line.level, LogLevel::Info);
        assert_eq!(line.thread.as_deref(), Some("Server thread"));
        assert!(line.line.starts_with("Done (3.214s)"));
        assert!(is_startup_complete(&line.line));
    }

    #[test]
    fn parses_warn_error_levels() {
        let warn = parse_line("[05:24:11] [Server thread/WARN]: Can't keep up!", 1);
        assert_eq!(warn.level, LogLevel::Warn);
        let err = parse_line(
            "[05:24:11] [main/FATAL]: Failed to start the minecraft server",
            2,
        );
        assert_eq!(err.level, LogLevel::Error);
    }

    #[test]
    fn unstructured_lines_fall_back_to_unknown() {
        let line = parse_line("java.lang.RuntimeException: boom", 0);
        assert_eq!(line.level, LogLevel::Unknown);
        assert!(line.thread.is_none());
        assert_eq!(line.line, "java.lang.RuntimeException: boom");
    }

    #[test]
    fn ansi_coloring_is_stripped() {
        let line = parse_line("\u{1b}[0;32;1m[05:24:11 INFO]: Done (3.214s)!\u{1b}[m", 0);
        assert_eq!(line.line, "[05:24:11 INFO]: Done (3.214s)!");
    }

    #[test]
    fn startup_and_stopping_signatures() {
        assert!(is_startup_complete(
            "Done (12.345s)! For help, type \"help\""
        ));
        assert!(!is_startup_complete("Starting minecraft server"));
        assert!(is_stopping("Stopping server"));
        assert!(!is_stopping("Server started"));
    }

    #[test]
    fn ring_is_bounded_and_counts() {
        let mut ring: Ring<u64> = Ring::new(3);
        for i in 0..5 {
            ring.push(i);
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.total_pushed(), 5);
        let seen: Vec<_> = ring.iter().copied().collect();
        assert_eq!(seen, vec![2, 3, 4], "oldest are evicted");
    }
}
