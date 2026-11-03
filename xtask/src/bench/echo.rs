//! `cargo xtask bench echo`: TCP echo throughput and latency benchmark client.
//!
//! Measures echo throughput and round-trip latency across TCP echo servers
//! (`rust-threads` and `loom`), printing results as a Markdown table.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::workspace_root;

const HELP: &str = "\
cargo xtask bench echo [OPTIONS]

Measures TCP echo throughput and latency across echo servers.

OPTIONS:
  --conns C            Number of concurrent connections (default: 100)
  --msgs M             Number of messages per connection (default: 1000)
  --size B             Message size in bytes (default: 64)
  --duration-secs D    Duration limit in seconds (default: none)
  --server NAME        Target server: rust-threads, loom (can be repeated)
  --help, -h           Print this help
";

/// Configuration options for `cargo xtask bench echo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Number of concurrent connections.
    pub conns: usize,
    /// Number of sequential messages per connection.
    pub msgs: usize,
    /// Payload size in bytes per message.
    pub size: usize,
    /// Optional execution duration limit in seconds.
    pub duration_secs: Option<u64>,
    /// Servers to benchmark. Empty implies all available servers.
    pub servers: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            conns: 100,
            msgs: 1000,
            size: 64,
            duration_secs: None,
            servers: Vec::new(),
        }
    }
}

/// Results of running the echo client workload.
#[derive(Clone, Debug)]
pub struct ClientResult {
    /// Total number of messages successfully sent and echoed back.
    pub total_messages: usize,
    /// Total wall-clock time elapsed across all connection threads.
    pub wall_time: Duration,
    /// Sorted message round-trip latencies.
    pub latencies: Vec<Duration>,
}

/// Benchmark row for a server in the final report table.
#[derive(Clone, Debug)]
pub struct ServerBenchRow {
    /// Server identifier name.
    pub server_name: String,
    /// Number of concurrent connections.
    pub connections: usize,
    /// Message size in bytes.
    pub message_size: usize,
    /// Total messages echoed.
    pub total_messages: usize,
    /// Total wall time.
    pub wall_time: Duration,
    /// Messages per second.
    pub msgs_per_sec: f64,
    /// Median latency (p50).
    pub p50: Duration,
    /// 99th percentile latency (p99).
    pub p99: Duration,
    /// Maximum latency.
    pub max: Duration,
}

struct ServerGuard(std::process::Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs the TCP echo benchmark subcommand.
///
/// # Errors
/// Returns an error if argument parsing, server compilation, connection, or byte verification fails.
pub fn run(args: &[String]) -> Result<(), String> {
    if args
        .iter()
        .any(|a| a == "--help" || a == "-h" || a == "help")
    {
        print!("{HELP}");
        return Ok(());
    }

    let options = parse_args(args)?;
    let root = workspace_root();
    let scratch = root.join("target").join("bench");
    std::fs::create_dir_all(&scratch).map_err(|e| {
        format!(
            "cannot create scratch directory `{}`: {e}",
            scratch.display()
        )
    })?;

    let servers_to_run = if options.servers.is_empty() {
        vec!["rust-threads".to_string(), "loom".to_string()]
    } else {
        options.servers.clone()
    };

    let mut rows = Vec::new();
    for server in &servers_to_run {
        match server.as_str() {
            "rust-threads" => {
                let exe = compile_rust_threads(&root, &scratch)?;
                let row = run_single_server("rust-threads", &exe, &options)?;
                rows.push(row);
            }
            "loom" => {
                if let Some(exe) = compile_loom(&root, &scratch)? {
                    let row = run_single_server("loom", &exe, &options)?;
                    rows.push(row);
                }
            }
            other => {
                return Err(format!(
                    "unknown server `{other}` (available: rust-threads, loom)"
                ));
            }
        }
    }

    if !rows.is_empty() {
        print_report(&rows);
    }

    Ok(())
}

fn compile_rust_threads(root: &Path, scratch: &Path) -> Result<PathBuf, String> {
    let src = root
        .join("benches")
        .join("echo")
        .join("reference_server.rs");
    if !src.exists() {
        return Err(format!("reference server not found at `{}`", src.display()));
    }
    let exe_name = if cfg!(windows) {
        "reference_server.exe"
    } else {
        "reference_server"
    };
    let exe_path = scratch.join(exe_name);
    let output = Command::new("rustc")
        .arg("-O")
        .arg(&src)
        .arg("-o")
        .arg(&exe_path)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run `rustc`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "rustc compilation failed for `{}`: {}",
            src.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(exe_path)
}

fn compile_loom(root: &Path, scratch: &Path) -> Result<Option<PathBuf>, String> {
    let src = root.join("benches").join("echo").join("echo_server.loom");
    if !src.exists() {
        println!("echo: skipping loom (benches/echo/echo_server.loom not found)");
        return Ok(None);
    }
    let loom_bin = super::build_loom_release(root)?;
    let exe_name = if cfg!(windows) {
        "echo_server_loom.exe"
    } else {
        "echo_server_loom"
    };
    let exe_path = scratch.join(exe_name);
    let output = Command::new(&loom_bin)
        .arg("build")
        .arg("-o")
        .arg(&exe_path)
        .arg(&src)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run `loom build`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "loom build failed for `{}`: {}",
            src.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Some(exe_path))
}

fn pick_free_port(server: &str) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("{server}: failed to bind ephemeral port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("{server}: failed to get local address: {e}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_for_listening(stdout: std::process::ChildStdout, server: &str) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(Err(
                        "server closed stdout before printing 'listening'".to_string()
                    ));
                    return;
                }
                Ok(_) => {
                    if line.trim() == "listening" || line.contains("listening") {
                        let _ = tx.send(Ok(()));
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("error reading server stdout: {e}")));
                    return;
                }
            }
        }
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(res) => res.map_err(|e| format!("{server}: {e}")),
        Err(_) => Err(format!(
            "{server}: timed out waiting for 'listening' (10 s)"
        )),
    }
}

fn run_single_server(
    server: &str,
    exe_path: &Path,
    options: &Options,
) -> Result<ServerBenchRow, String> {
    let port = pick_free_port(server)?;
    let mut child = Command::new(exe_path)
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{server}: failed to spawn server: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{server}: failed to capture server stdout"))?;

    if let Err(err) = wait_for_listening(stdout, server) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }

    let guard = ServerGuard(child);
    let client_res = run_client_on_port(server, port, options)?;
    drop(guard);

    #[allow(clippy::cast_precision_loss)]
    // Total message count fits comfortably within f64 mantissa (up to 2^53).
    let msgs_per_sec = if client_res.wall_time.as_secs_f64() > 0.0 {
        client_res.total_messages as f64 / client_res.wall_time.as_secs_f64()
    } else {
        0.0
    };

    let p50 = percentile(&client_res.latencies, 50.0);
    let p99 = percentile(&client_res.latencies, 99.0);
    let max = client_res
        .latencies
        .last()
        .copied()
        .unwrap_or(Duration::ZERO);

    Ok(ServerBenchRow {
        server_name: server.to_string(),
        connections: options.conns,
        message_size: options.size,
        total_messages: client_res.total_messages,
        wall_time: client_res.wall_time,
        msgs_per_sec,
        p50,
        p99,
        max,
    })
}

/// Runs the benchmark client against an echo server listening on `127.0.0.1:port`.
///
/// # Errors
/// Returns an error if connecting, writing, reading, or byte verification fails.
pub fn run_client_on_port(
    server: &str,
    port: u16,
    options: &Options,
) -> Result<ClientResult, String> {
    let deadline = options
        .duration_secs
        .map(|d| Instant::now() + Duration::from_secs(d));
    let wall_start = Instant::now();

    let thread_latencies: Vec<Vec<Duration>> =
        std::thread::scope(|scope| -> Result<Vec<Vec<Duration>>, String> {
            let mut handles = Vec::with_capacity(options.conns);
            for k in 0..options.conns {
                handles
                    .push(scope.spawn(move || run_connection(server, port, k, options, deadline)));
            }

            let mut results = Vec::with_capacity(options.conns);
            for handle in handles {
                let res = handle
                    .join()
                    .map_err(|_| format!("{server}: connection thread panicked"))??;
                results.push(res);
            }
            Ok(results)
        })?;

    let wall_time = wall_start.elapsed();
    let mut all_latencies = Vec::new();
    for l in thread_latencies {
        all_latencies.extend(l);
    }
    all_latencies.sort();
    let total_messages = all_latencies.len();

    Ok(ClientResult {
        total_messages,
        wall_time,
        latencies: all_latencies,
    })
}

fn run_connection(
    server: &str,
    port: u16,
    conn_idx: usize,
    options: &Options,
    deadline: Option<Instant>,
) -> Result<Vec<Duration>, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("{server}: connection {conn_idx} failed to connect: {e}"))?;
    let _ = stream.set_nodelay(true);

    let mut recv_buf = vec![0u8; options.size];
    let mut latencies = Vec::with_capacity(options.msgs);

    for msg_idx in 0..options.msgs {
        if let Some(dl) = deadline
            && Instant::now() >= dl
        {
            break;
        }
        let send_buf = byte_pattern(conn_idx, msg_idx, options.size);
        let send_start = Instant::now();
        stream.write_all(&send_buf).map_err(|e| {
            format!("{server}: connection {conn_idx} write failed on message {msg_idx}: {e}")
        })?;
        stream.read_exact(&mut recv_buf).map_err(|e| {
            format!("{server}: connection {conn_idx} read failed on message {msg_idx}: {e}")
        })?;
        let rtt = send_start.elapsed();
        if recv_buf != send_buf {
            return Err(format!(
                "{server}: echo mismatch on connection {conn_idx}, message {msg_idx}"
            ));
        }
        latencies.push(rtt);
    }

    Ok(latencies)
}

/// Generates a byte pattern vector of length `size` derived from `conn` and `msg`.
#[must_use]
pub fn byte_pattern(conn: usize, msg: usize, size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    fill_byte_pattern(conn, msg, &mut buf);
    buf
}

/// Fills `buf` with a deterministic byte pattern derived from connection index `conn`
/// and message index `msg`.
pub fn fill_byte_pattern(conn: usize, msg: usize, buf: &mut [u8]) {
    let k = conn as u64;
    let i = msg as u64;
    for (j, byte) in buf.iter_mut().enumerate() {
        let j = j as u64;
        let val = k.wrapping_mul(10007) ^ i.wrapping_mul(37) ^ j.wrapping_mul(13) ^ ((k ^ i) << 3);
        *byte = ((val ^ (val >> 8)) & 0xFF) as u8;
    }
}

/// Computes the `p`-th percentile (0.0 to 100.0) of a sorted duration slice
/// using the nearest-rank method.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
// Truncation and precision loss are bounded by the slice length, which fits in f64 mantissa.
pub fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    if p <= 0.0 {
        return sorted[0];
    }
    if p >= 100.0 {
        return *sorted.last().unwrap();
    }
    let n = sorted.len() as f64;
    let rank = ((p / 100.0) * n).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

/// Parses CLI arguments for `cargo xtask bench echo`.
///
/// # Errors
/// Returns an error if unknown options or invalid values are provided.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut iter = args.iter().peekable();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--conns" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--conns requires a number argument".to_string())?;
                options.conns = parse_positive(val, "--conns")?;
            }
            s if s.starts_with("--conns=") => {
                let val = &s["--conns=".len()..];
                options.conns = parse_positive(val, "--conns")?;
            }
            "--msgs" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--msgs requires a number argument".to_string())?;
                options.msgs = parse_positive(val, "--msgs")?;
            }
            s if s.starts_with("--msgs=") => {
                let val = &s["--msgs=".len()..];
                options.msgs = parse_positive(val, "--msgs")?;
            }
            "--size" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--size requires a number argument".to_string())?;
                options.size = parse_positive(val, "--size")?;
            }
            s if s.starts_with("--size=") => {
                let val = &s["--size=".len()..];
                options.size = parse_positive(val, "--size")?;
            }
            "--duration-secs" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--duration-secs requires a number argument".to_string())?;
                let d = parse_positive::<u64>(val, "--duration-secs")?;
                options.duration_secs = Some(d);
            }
            s if s.starts_with("--duration-secs=") => {
                let val = &s["--duration-secs=".len()..];
                let d = parse_positive::<u64>(val, "--duration-secs")?;
                options.duration_secs = Some(d);
            }
            "--server" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--server requires a server name argument".to_string())?;
                validate_server(val)?;
                options.servers.push(val.clone());
            }
            s if s.starts_with("--server=") => {
                let val = &s["--server=".len()..];
                validate_server(val)?;
                options.servers.push(val.to_string());
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    Ok(options)
}

fn parse_positive<T: std::str::FromStr + PartialOrd + Default>(
    val: &str,
    flag: &str,
) -> Result<T, String> {
    let parsed = val
        .parse::<T>()
        .map_err(|_| format!("invalid value for {flag}: `{val}`"))?;
    if parsed <= T::default() {
        return Err(format!("{flag} must be greater than 0"));
    }
    Ok(parsed)
}

fn validate_server(name: &str) -> Result<(), String> {
    match name {
        "rust-threads" | "loom" => Ok(()),
        other => Err(format!(
            "unknown server `{other}` (available: rust-threads, loom)"
        )),
    }
}

fn print_report(rows: &[ServerBenchRow]) {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    println!(
        "Machine: {}/{}, {} logical CPUs\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cpus
    );
    println!(
        "| Server | Connections | Message Size | Total Messages | Wall Time | Messages/s | p50 | p99 | Max |"
    );
    println!("| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |");
    for row in rows {
        let wall_secs = row.wall_time.as_secs_f64();
        let p50_us = row.p50.as_secs_f64() * 1_000_000.0;
        let p99_us = row.p99.as_secs_f64() * 1_000_000.0;
        let max_us = row.max.as_secs_f64() * 1_000_000.0;
        println!(
            "| {} | {} | {} B | {} | {:.2} s | {:.1} | {:.1} µs | {:.1} µs | {:.1} µs |",
            row.server_name,
            row.connections,
            row.message_size,
            row.total_messages,
            wall_secs,
            row.msgs_per_sec,
            p50_us,
            p99_us,
            max_us
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_parse_args_defaults() {
        let options = parse_args(&[]).unwrap();
        assert_eq!(
            options,
            Options {
                conns: 100,
                msgs: 1000,
                size: 64,
                duration_secs: None,
                servers: Vec::new(),
            }
        );
    }

    #[test]
    fn test_parse_args_custom() {
        let args = vec![
            "--conns".to_string(),
            "50".to_string(),
            "--msgs".to_string(),
            "200".to_string(),
            "--size".to_string(),
            "128".to_string(),
            "--duration-secs".to_string(),
            "5".to_string(),
            "--server".to_string(),
            "rust-threads".to_string(),
        ];
        let options = parse_args(&args).unwrap();
        assert_eq!(
            options,
            Options {
                conns: 50,
                msgs: 200,
                size: 128,
                duration_secs: Some(5),
                servers: vec!["rust-threads".to_string()],
            }
        );
    }

    #[test]
    fn test_parse_args_equal_syntax() {
        let args = vec![
            "--conns=20".to_string(),
            "--msgs=300".to_string(),
            "--size=32".to_string(),
            "--duration-secs=10".to_string(),
            "--server=loom".to_string(),
            "--server=rust-threads".to_string(),
        ];
        let options = parse_args(&args).unwrap();
        assert_eq!(
            options,
            Options {
                conns: 20,
                msgs: 300,
                size: 32,
                duration_secs: Some(10),
                servers: vec!["loom".to_string(), "rust-threads".to_string()],
            }
        );
    }

    #[test]
    fn test_parse_args_validation_errors() {
        assert!(parse_args(&["--conns".to_string(), "0".to_string()]).is_err());
        assert!(parse_args(&["--msgs".to_string(), "0".to_string()]).is_err());
        assert!(parse_args(&["--size".to_string(), "0".to_string()]).is_err());
        assert!(parse_args(&["--duration-secs".to_string(), "0".to_string()]).is_err());
        assert!(parse_args(&["--conns".to_string(), "abc".to_string()]).is_err());
        assert!(parse_args(&["--server".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_args(&["--unknown".to_string()]).is_err());
    }

    #[test]
    fn test_byte_pattern_properties() {
        let p1 = byte_pattern(0, 0, 64);
        let p2 = byte_pattern(0, 0, 64);
        assert_eq!(p1, p2, "pattern must be deterministic");
        assert_eq!(p1.len(), 64);

        let p_diff_conn = byte_pattern(1, 0, 64);
        assert_ne!(
            p1, p_diff_conn,
            "different connections must yield different patterns"
        );

        let p_diff_msg = byte_pattern(0, 1, 64);
        assert_ne!(
            p1, p_diff_msg,
            "different messages must yield different patterns"
        );

        let p_empty = byte_pattern(5, 5, 0);
        assert_eq!(p_empty, [] as [u8; 0]);
    }

    #[test]
    fn test_percentile_computation() {
        assert_eq!(percentile(&[], 50.0), Duration::ZERO);

        let single = [Duration::from_micros(42)];
        assert_eq!(percentile(&single, 0.0), Duration::from_micros(42));
        assert_eq!(percentile(&single, 50.0), Duration::from_micros(42));
        assert_eq!(percentile(&single, 100.0), Duration::from_micros(42));

        let samples: Vec<Duration> = (1..=100).map(Duration::from_micros).collect();
        assert_eq!(percentile(&samples, 50.0), Duration::from_micros(50));
        assert_eq!(percentile(&samples, 99.0), Duration::from_micros(99));
        assert_eq!(percentile(&samples, 100.0), Duration::from_micros(100));
        assert_eq!(percentile(&samples, 0.0), Duration::from_micros(1));
    }

    #[test]
    fn test_echo_client_integration() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().expect("local addr").port();
        listener.set_nonblocking(true).expect("set nonblocking");

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        let server_thread = std::thread::spawn(move || {
            let mut client_threads = Vec::new();
            while !stop_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // On macOS (BSD) and Windows, accepted sockets inherit O_NONBLOCK
                        // from the listening socket. Explicitly restore blocking mode for
                        // the worker thread.
                        let _ = stream.set_nonblocking(false);
                        let handle = std::thread::spawn(move || {
                            let _ = stream.set_nodelay(true);
                            let mut buf = [0u8; 1024];
                            loop {
                                match stream.read(&mut buf) {
                                    Ok(0) | Err(_) => break,
                                    Ok(n) => {
                                        if stream.write_all(&buf[..n]).is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                        client_threads.push(handle);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for handle in client_threads {
                let _ = handle.join();
            }
        });

        let options = Options {
            conns: 4,
            msgs: 50,
            size: 64,
            duration_secs: None,
            servers: vec!["in-process".to_string()],
        };

        let result = run_client_on_port("in-process", port, &options);
        stop.store(true, Ordering::Relaxed);
        let _ = server_thread.join();

        let client_res = result.expect("in-process echo client benchmark should succeed");
        assert_eq!(client_res.total_messages, 4 * 50);
        assert_eq!(client_res.latencies.len(), 4 * 50);
        assert!(client_res.wall_time > Duration::ZERO);
    }
}
