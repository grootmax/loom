//! Binary entry point for `loom_fuzz`.

use std::process;

use loom_fuzz::cli::Options;
use loom_fuzz::{run_fuzzer, run_replay};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let options = match Options::parse_args(args) {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };

    if let Some(ref replay_file) = options.replay_file {
        if let Err(msg) = run_replay(replay_file, options.target) {
            eprintln!("replay failed: {msg}");
            process::exit(1);
        }
        return;
    }

    match run_fuzzer(&options) {
        Ok(failures) => {
            if failures > 0 {
                process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("fuzzer error: {err}");
            process::exit(1);
        }
    }
}
