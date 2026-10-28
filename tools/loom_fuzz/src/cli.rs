//! Command line options parsing for `loom_fuzz`.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Fuzzing target component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzTarget {
    /// Fuzz the lexer (`loom_lexer`).
    Lex,
    /// Fuzz the parser (`loom_parser`).
    Parse,
    /// Fuzz the formatter (`loom_fmt`).
    Fmt,
    /// Fuzz all front-end components.
    All,
}

impl FuzzTarget {
    /// Parses a target name string.
    ///
    /// # Errors
    ///
    /// Returns an error message if the name is invalid.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "lex" => Ok(Self::Lex),
            "parse" => Ok(Self::Parse),
            "fmt" => Ok(Self::Fmt),
            "all" => Ok(Self::All),
            _ => Err(format!(
                "unknown target `{s}`; expected lex, parse, fmt, or all"
            )),
        }
    }

    /// Returns the string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lex => "lex",
            Self::Parse => "parse",
            Self::Fmt => "fmt",
            Self::All => "all",
        }
    }
}

/// Command line configuration options.
#[derive(Clone, Debug)]
pub struct Options {
    /// Target component(s) to fuzz.
    pub target: FuzzTarget,
    /// Maximum execution time in seconds.
    pub seconds: Option<u64>,
    /// Maximum number of fuzzing iterations.
    pub iterations: Option<u64>,
    /// Seed for the PRNG.
    pub seed: u64,
    /// Corpus directories containing `.loom` input files.
    pub corpus_dirs: Vec<PathBuf>,
    /// Directory where failing inputs are written.
    pub out_dir: PathBuf,
    /// Optional path to a file to replay.
    pub replay_file: Option<PathBuf>,
}

impl Options {
    /// Parses CLI arguments.
    ///
    /// # Errors
    ///
    /// Returns an error string if options cannot be parsed or are invalid.
    pub fn parse_args<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut target = FuzzTarget::All;
        let mut seconds = Some(10);
        let mut iterations = None;
        let mut seed = None;
        let mut corpus_dirs = Vec::new();
        let mut out_dir = PathBuf::from("target/fuzz");
        let mut replay_file = None;

        let mut iter = args.into_iter().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--target" => {
                    let val = iter.next().ok_or("missing value for --target")?;
                    target = FuzzTarget::parse(&val)?;
                }
                "--seconds" => {
                    let val = iter.next().ok_or("missing value for --seconds")?;
                    let secs: u64 = val.parse().map_err(|_| "invalid number for --seconds")?;
                    seconds = Some(secs);
                }
                "--iterations" => {
                    let val = iter.next().ok_or("missing value for --iterations")?;
                    let iters: u64 = val.parse().map_err(|_| "invalid number for --iterations")?;
                    iterations = Some(iters);
                }
                "--seed" => {
                    let val = iter.next().ok_or("missing value for --seed")?;
                    let s: u64 = val.parse().map_err(|_| "invalid seed number")?;
                    seed = Some(s);
                }
                "--corpus" => {
                    let val = iter.next().ok_or("missing value for --corpus")?;
                    corpus_dirs.push(PathBuf::from(val));
                }
                "--out" => {
                    let val = iter.next().ok_or("missing value for --out")?;
                    out_dir = PathBuf::from(val);
                }
                "--replay" => {
                    let val = iter.next().ok_or("missing value for --replay")?;
                    replay_file = Some(PathBuf::from(val));
                }
                "--help" | "-h" => {
                    return Err(
                        "Usage: loom_fuzz [--target lex|parse|fmt|all] [--seconds N] \
                         [--iterations N] [--seed S] [--corpus DIR]... [--out DIR] \
                         [--replay FILE]"
                            .to_string(),
                    );
                }
                other => {
                    return Err(format!("unknown argument `{other}`"));
                }
            }
        }

        let seed = seed.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(d.as_secs()))
        });

        if corpus_dirs.is_empty() {
            corpus_dirs = vec![
                PathBuf::from("examples"),
                PathBuf::from("spec/conformance"),
                PathBuf::from("tools/loom_fuzz/corpus/regressions"),
            ];
        }

        Ok(Self {
            target,
            seconds,
            iterations,
            seed,
            corpus_dirs,
            out_dir,
            replay_file,
        })
    }
}
