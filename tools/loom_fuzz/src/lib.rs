//! In-house fuzzer for the Loom front end.

pub mod cli;
pub mod corpus;
pub mod generate;
pub mod invariants;
pub mod minimize;
pub mod mutate;
pub mod prng;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub use cli::{FuzzTarget, Options};
pub use corpus::Corpus;
pub use invariants::{check_fmt, check_lex, check_parse, check_parse_bytes};
pub use minimize::minimize;
pub use prng::Rng;

/// Runs the fuzzer according to `options`. Returns the number of invariant failures.
///
/// # Errors
///
/// Returns an error message if corpus loading or writing failing inputs fails.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn run_fuzzer(options: &Options) -> Result<usize, String> {
    println!("seed={}", options.seed);

    let corpus = Corpus::load(&options.corpus_dirs);
    println!("loaded corpus with {} inputs", corpus.len());

    fs::create_dir_all(&options.out_dir).map_err(|e| {
        format!(
            "failed to create output directory {}: {e}",
            options.out_dir.display()
        )
    })?;

    // Silence default panic hook during fuzzing runs
    std::panic::set_hook(Box::new(|_| {}));

    let mut rng = Rng::new(options.seed);
    let start_time = Instant::now();
    let mut last_progress = Instant::now();

    let mut iterations: u64 = 0;
    let mut failures: usize = 0;

    loop {
        iterations += 1;

        // Choose target
        let target = match options.target {
            FuzzTarget::All => match rng.gen_range(0, 3) {
                0 => FuzzTarget::Lex,
                1 => FuzzTarget::Parse,
                _ => FuzzTarget::Fmt,
            },
            other => other,
        };

        // Select input source
        let strategy = rng.gen_range(0, 6);
        let input_res: Result<(), (String, String)> = match strategy {
            0 => {
                let seed_str = rng.choose(corpus.inputs());
                let mutated = mutate::mutate_bytes(seed_str, &mut rng);
                run_target_string(target, &mutated)
            }
            1 => {
                let seed_str = rng.choose(corpus.inputs());
                let mutated = mutate::mutate_tokens(seed_str, &mut rng);
                run_target_string(target, &mutated)
            }
            2 => {
                let seed_str = rng.choose(corpus.inputs());
                let mutated = mutate::mutate_lines(seed_str, &mut rng);
                run_target_string(target, &mutated)
            }
            3 => {
                let s1 = rng.choose(corpus.inputs());
                let s2 = rng.choose(corpus.inputs());
                let spliced = mutate::splice(s1, s2, &mut rng);
                run_target_string(target, &spliced)
            }
            4 => {
                let seed_str = rng.choose(corpus.inputs());
                let nested = mutate::nest(seed_str, &mut rng);
                run_target_string(target, &nested)
            }
            5 => {
                let generated = generate::generate(&mut rng);
                run_target_string(target, &generated)
            }
            _ => {
                // Arbitrary bytes for parse_bytes
                let len = rng.gen_range(1, 100);
                let mut bytes = Vec::with_capacity(len);
                for _ in 0..len {
                    bytes.push(rng.gen_byte());
                }
                run_parse_bytes(&bytes)
            }
        };

        if let Err((failing_input, err_msg)) = input_res {
            failures += 1;

            let minimized = minimize(&failing_input, |candidate| {
                std::panic::catch_unwind(|| check_target_string(target, candidate).is_err())
                    .unwrap_or(true)
            });

            let hash = short_hash(&minimized);
            let out_file = options
                .out_dir
                .join(format!("{}-{}.loom", target.as_str(), hash));

            let _ = fs::write(&out_file, &minimized);

            println!(
                "[FAIL] target: {}, invariant: {}, wrote: {}",
                target.as_str(),
                err_msg,
                out_file.display()
            );
        }

        // Report progress every 10 seconds
        let elapsed = last_progress.elapsed();
        if elapsed >= Duration::from_secs(10) {
            let total_secs = start_time.elapsed().as_secs_f64();
            let rate = if total_secs > 0.0 {
                (iterations as f64) / total_secs
            } else {
                0.0
            };
            println!("iteration {iterations}, inputs/sec {rate:.1}");
            last_progress = Instant::now();
        }

        // Termination criteria
        if let Some(max_secs) = options.seconds
            && start_time.elapsed().as_secs() >= max_secs
        {
            break;
        }
        if let Some(max_iters) = options.iterations
            && iterations >= max_iters
        {
            break;
        }
    }

    let total_secs = start_time.elapsed().as_secs_f64();
    let rate = if total_secs > 0.0 {
        (iterations as f64) / total_secs
    } else {
        0.0
    };
    println!(
        "fuzzing completed: {iterations} iterations ({rate:.1} inputs/sec), {failures} failures in {total_secs:.1}s"
    );

    Ok(failures)
}

/// Replays target checks on a single file.
///
/// # Errors
///
/// Returns an error if reading the file fails or any target invariant fails.
pub fn run_replay(file_path: &Path, target: FuzzTarget) -> Result<(), String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("failed to read replay file {}: {e}", file_path.display()))?;

    let check_res = match target {
        FuzzTarget::Lex => check_lex(&content).map_err(|e| e.to_string()),
        FuzzTarget::Parse => check_parse(&content).map_err(|e| e.to_string()),
        FuzzTarget::Fmt => check_fmt(&content).map_err(|e| e.to_string()),
        FuzzTarget::All => check_lex(&content)
            .and_then(|()| check_parse(&content))
            .and_then(|()| check_fmt(&content))
            .map_err(|e| e.to_string()),
    };

    match check_res {
        Ok(()) => {
            println!("[PASS] {}", file_path.display());
            Ok(())
        }
        Err(msg) => {
            println!("[FAIL] {}: {msg}", file_path.display());
            Err(msg)
        }
    }
}

fn check_target_string(target: FuzzTarget, input: &str) -> Result<(), String> {
    match target {
        FuzzTarget::Lex => check_lex(input).map_err(|e| e.to_string()),
        FuzzTarget::Parse => check_parse(input).map_err(|e| e.to_string()),
        FuzzTarget::Fmt => check_fmt(input).map_err(|e| e.to_string()),
        FuzzTarget::All => check_lex(input)
            .and_then(|()| check_parse(input))
            .and_then(|()| check_fmt(input))
            .map_err(|e| e.to_string()),
    }
}

fn run_target_string(target: FuzzTarget, input: &str) -> Result<(), (String, String)> {
    let input_clone = input.to_string();
    let res = std::panic::catch_unwind(|| check_target_string(target, &input_clone));

    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err_msg)) => Err((input_clone, err_msg)),
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "panic occurred".to_string()
            };
            Err((input_clone, format!("panic: {msg}")))
        }
    }
}

fn run_parse_bytes(bytes: &[u8]) -> Result<(), (String, String)> {
    let bytes_clone = bytes.to_vec();
    let res = std::panic::catch_unwind(|| check_parse_bytes(&bytes_clone));

    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            let lossy_text = String::from_utf8_lossy(&bytes_clone).into_owned();
            Err((lossy_text, err.to_string()))
        }
        Err(panic_payload) => {
            let lossy_text = String::from_utf8_lossy(&bytes_clone).into_owned();
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "panic occurred".to_string()
            };
            Err((lossy_text, format!("panic in parse_bytes: {msg}")))
        }
    }
}

fn short_hash(data: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:012x}")
}
