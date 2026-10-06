//! fake-mc-server: a deterministic mimic of the Paper lifecycle, used by
//! supervisor and daemon tests so the lifecycle matrix runs without Java
//! (TESTING.md). Modes are composable flags; the process prints
//! Paper-shaped stdout, consumes stdin commands, and exits on cue.
//!
//! Like the real thing, it owns `logs/latest.log` in its working directory
//! (the server root): the file is created fresh at boot — Paper rotates the
//! previous session away — and every emitted line lands in both stdout and
//! the file, so "the log files hold the full history" (ADR-0006) holds for
//! mimicked servers exactly as it does for real ones.

use std::io::{BufRead, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

struct Flags {
    boot_ms: u64,
    fail_boot: bool,
    exit_after_boot: bool,
    crash_mid_run: bool,
    exit_code: i32,
    ignore_stop: bool,
    slow_stop_ms: u64,
    flood_stdout: u64,
    port: Option<u16>,
}

fn parse_flags() -> Flags {
    let mut flags = Flags {
        boot_ms: 100,
        fail_boot: false,
        exit_after_boot: false,
        crash_mid_run: false,
        exit_code: 0,
        ignore_stop: false,
        slow_stop_ms: 0,
        flood_stdout: 0,
        port: None,
    };
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let value_for = |i: &mut usize, name: &str| -> u64 {
            *i += 1;
            let raw = args
                .get(*i)
                .unwrap_or_else(|| panic!("{name} needs a value"))
                .clone();
            raw.parse()
                .unwrap_or_else(|_| panic!("{name} must be a number, got {raw:?}"))
        };
        match arg.as_str() {
            // Accept the exact argv shape the supervisor uses for a real
            // server, so tests exercise the genuine spawn path.
            "-Xms" | "-Xmx" => {
                let _ = value_for(&mut i, &arg);
            }
            "-jar" => {
                // Consume the jar path; the mimic never reads it.
                i += 1;
            }
            "nogui" => {}
            // The Java inspection probe: answer like a JVM would, on
            // stderr, and exit before the server lifecycle begins.
            "-XshowSettings:properties" => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(
                    stderr,
                    "  java.version = 21.0.3\n  java.vendor = Fake Temurin\n  java.home = /fake"
                );
                std::process::exit(0);
            }
            "-version" => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "openjdk version \"21.0.3\" 2026-01-01");
                std::process::exit(0);
            }
            other if other.starts_with("--") => match other {
                "--boot-ms" => flags.boot_ms = value_for(&mut i, other),
                "--exit-code" => flags.exit_code = value_for(&mut i, other) as i32,
                "--slow-stop-ms" => flags.slow_stop_ms = value_for(&mut i, other),
                "--flood-stdout" => flags.flood_stdout = value_for(&mut i, other),
                "--port" => flags.port = Some(value_for(&mut i, other) as u16),
                "--fail-boot" => flags.fail_boot = true,
                "--exit-after-boot" => flags.exit_after_boot = true,
                "--crash-mid-run" => flags.crash_mid_run = true,
                "--ignore-stop" => flags.ignore_stop = true,
                _ => panic!("unknown flag {other}"),
            },
            other => panic!("unknown argument {other}"),
        }
        i += 1;
    }
    flags
}

fn emit(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
    if let Some(file) = session_log() {
        if let Ok(mut handle) = file.lock() {
            let _ = writeln!(handle, "{line}");
            // Paper's appender flushes per line; the mimic does too, so
            // file-backed reads see a live session's lines immediately.
            let _ = handle.flush();
        }
    }
}

/// The session's `logs/latest.log`, created fresh on first emit (boot).
/// Unwritable roots (no directory, read-only) degrade to stdout-only.
fn session_log() -> Option<&'static Mutex<std::fs::File>> {
    static LOG: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    LOG.get_or_init(|| {
        std::fs::create_dir_all("logs").ok()?;
        std::fs::File::create("logs/latest.log")
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Paper-family stdout carries a local-time prefix:
/// `[HH:MM:SS] [Thread/LEVEL]: message`. Tests downstream (log parsing,
/// the log viewer) depend on the shape being faithful.
fn paper_line(thread_level: &str, message: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86_400;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("[{h:02}:{m:02}:{s:02}] [{thread_level}]: {message}")
}

fn info(msg: &str) {
    emit(&paper_line("Server thread/INFO", msg));
}

fn main() {
    let flags = parse_flags();

    // Hold the port like a real server would, so the supervisor's
    // port-listening validation observes the real thing.
    let _listener = flags.port.map(|port| {
        TcpListener::bind(("127.0.0.1", port))
            .unwrap_or_else(|e| panic!("cannot bind port {port}: {e}"))
    });

    info("Starting minecraft server version 1.21.1");
    info("Loading properties");
    if flags.fail_boot {
        emit(&paper_line(
            "main/FATAL",
            "Failed to start the minecraft server",
        ));
        std::process::exit(flags.exit_code);
    }
    thread::sleep(Duration::from_millis(flags.boot_ms));
    info("Done (1.234s)! For help, type \"help\"");

    if flags.exit_after_boot {
        std::process::exit(flags.exit_code);
    }

    let stopping = Arc::new(AtomicBool::new(false));

    // stdin owns the graceful path: "stop" ends the process, "list" proves
    // command round trips through the supervisor's stdin pipe.
    {
        let stopping = Arc::clone(&stopping);
        let ignore_stop = flags.ignore_stop;
        let slow_stop_ms = flags.slow_stop_ms;
        thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { return };
                match line.trim() {
                    "stop" => {
                        if ignore_stop {
                            info("Ignoring stop (test mode)");
                            continue;
                        }
                        stopping.store(true, Ordering::SeqCst);
                        info("Stopping server");
                        thread::sleep(Duration::from_millis(slow_stop_ms));
                        std::process::exit(0);
                    }
                    "list" => info("There are 0 of a max of 20 players online"),
                    "" => {}
                    other => info(&format!("Unknown command: {other}")),
                }
            }
        });
    }

    if flags.crash_mid_run {
        thread::sleep(Duration::from_millis(300));
        emit(&paper_line(
            "Server thread/ERROR",
            "Encountered an unexpected exception",
        ));
        std::process::exit(flags.exit_code);
    }

    // Output flood mode paces itself so tests can measure throughput.
    if flags.flood_stdout > 0 {
        let per_line = Duration::from_nanos(1_000_000_000 / flags.flood_stdout.max(1));
        let mut i: u64 = 0;
        loop {
            if stopping.load(Ordering::SeqCst) {
                break;
            }
            emit(&paper_line(
                "Server thread/INFO",
                &format!("flood line {i} with some padding text 0123456789"),
            ));
            i += 1;
            thread::sleep(per_line);
        }
        std::process::exit(0);
    }

    loop {
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    std::process::exit(0);
}
