//! The `loom` command-line tool.
//!
//! Status: Phase 1. `check` (syntax only), `fmt` and `explain` work; every other command
//! reports which roadmap phase will implement it and exits with status 2.

mod link;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use loom_diagnostics::Diagnostic;
use loom_driver::Session;

mod explain;

/// Output format for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum MessageFormat {
    /// Rendered rustc-style snippets with summary line.
    #[default]
    Rendered,
    /// One-line diagnostic output (`file:line:col: severity[code]: message`).
    Short,
}

/// Control whether color escape codes are emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum ColorChoice {
    /// Auto-detect based on stderr terminal and `NO_COLOR` environment variable.
    #[default]
    Auto,
    /// Always emit color escape codes.
    Always,
    /// Never emit color escape codes.
    Never,
}

impl ColorChoice {
    fn should_color(self) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                std::env::var_os("NO_COLOR").is_none()
                    && std::io::IsTerminal::is_terminal(&std::io::stderr())
            }
        }
    }
}

/// A `loom` subcommand and the roadmap phase that delivers it.
struct Command {
    name: &'static str,
    summary: &'static str,
    phase: u8,
}

const COMMANDS: &[Command] = &[
    cmd("new", "Create a new project from a template", 5),
    cmd("init", "Create a project in the current directory", 5),
    cmd("run", "Build and run a program", 2),
    cmd("build", "Compile a program to a native binary", 3),
    cmd("check", "Check programs for errors", 1),
    cmd("test", "Run tests, including doc-tests", 2),
    cmd("bench", "Run benchmarks", 6),
    cmd("fmt", "Format source files in the canonical style", 1),
    cmd("lint", "Run the built-in linter", 6),
    cmd("doc", "Generate HTML documentation", 6),
    cmd("repl", "Start an interactive shell", 6),
    cmd("add", "Add a dependency", 5),
    cmd("remove", "Remove a dependency", 5),
    cmd("update", "Update dependencies", 5),
    cmd("vendor", "Vendor dependencies into the project", 5),
    cmd("publish", "Publish a package", 5),
    cmd("lsp", "Start the language server", 6),
    cmd("fix", "Apply automated migrations", 7),
    cmd("explain", "Explain an error code in detail", 1),
    cmd("env", "Print toolchain and environment information", 5),
    cmd("doctor", "Diagnose problems with the installation", 5),
];

const fn cmd(name: &'static str, summary: &'static str, phase: u8) -> Command {
    Command {
        name,
        summary,
        phase,
    }
}

/// Exit status for usage errors (unknown command or flag).
const USAGE: u8 = 2;

fn print_help() {
    println!(
        "loom {} — the Loom toolchain (pre-alpha)",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE: loom <command> [args]");
    println!();
    println!("COMMANDS:");
    for command in COMMANDS {
        println!(
            "  {:<10} {} (phase {})",
            command.name, command.summary, command.phase
        );
    }
    println!("  {:<10} Print this help", "help");
    println!();
    println!("OPTIONS:");
    println!("  -V, --version  Print the version");
    println!();
    println!("  loom check [--dump=cst] [--message-format=short] <paths>...");
    println!("  loom fmt [--check] [<paths>...]");
    println!("  loom explain <code>");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        print_help();
        return ExitCode::SUCCESS;
    };
    let rest = &args[1..];
    match first.as_str() {
        "-V" | "--version" | "version" => {
            println!("loom {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "-h" | "--help" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        "check" => check(rest),
        "run" => run(rest, false),
        "test" => run(rest, true),
        "build" => build(rest),
        "fmt" => fmt(rest),
        "explain" => explain(rest),
        name => {
            if let Some(command) = COMMANDS.iter().find(|command| command.name == name) {
                eprintln!(
                    "loom {}: not implemented yet (planned for phase {}; see docs/roadmap.md)",
                    command.name, command.phase
                );
            } else {
                eprintln!(
                    "loom: unknown command `{name}`. Run `loom help` for a list of commands."
                );
            }
            ExitCode::from(USAGE)
        }
    }
}

/// Splits `--flag` / `--flag=value` options from paths.
fn split_options(args: &[String]) -> (Vec<&str>, Vec<PathBuf>) {
    let mut options = Vec::new();
    let mut paths = Vec::new();
    for arg in args {
        if arg.starts_with("--") {
            options.push(arg.as_str());
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    (options, paths)
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("loom: {message}");
    ExitCode::from(USAGE)
}

/// Loads every `.loom` file under `paths` into a session.
fn load(paths: &[PathBuf]) -> Result<Session, ExitCode> {
    let files = loom_driver::collect_sources(paths).map_err(|error| {
        eprintln!("loom: {error}");
        ExitCode::from(USAGE)
    })?;
    let mut session = Session::new();
    for file in files {
        if let Err(error) = session.load(&file) {
            eprintln!("loom: {error}");
            return Err(ExitCode::FAILURE);
        }
    }
    Ok(session)
}

fn summary_line(file_path: &Path, errors: usize, warnings: usize, color: bool) -> Option<String> {
    if errors == 0 && warnings == 0 {
        return None;
    }

    let (bold_red, bold, reset) = if color {
        ("\x1b[1;31m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "")
    };

    if errors > 0 {
        let error_text = if errors == 1 {
            "1 previous error".to_string()
        } else {
            format!("{errors} previous errors")
        };
        let warning_text = if warnings == 0 {
            String::new()
        } else if warnings == 1 {
            "; 1 warning emitted".to_string()
        } else {
            format!("; {warnings} warnings emitted")
        };
        Some(format!(
            "{bold_red}error{reset}{bold}: could not check `{}` due to {error_text}{warning_text}{reset}",
            file_path.display()
        ))
    } else {
        let (bold_yellow, reset) = if color {
            ("\x1b[1;33m", "\x1b[0m")
        } else {
            ("", "")
        };
        let warning_text = if warnings == 1 {
            "1 warning emitted".to_string()
        } else {
            format!("{warnings} warnings emitted")
        };
        Some(format!(
            "{bold_yellow}warning{reset}{bold}: {warning_text}{reset}"
        ))
    }
}

fn print_diagnostics_for_file(
    diagnostics: &[Diagnostic],
    sources: &loom_span::SourceMap,
    path: &Path,
    format: MessageFormat,
    color: ColorChoice,
) {
    if diagnostics.is_empty() {
        return;
    }
    let stderr = std::io::stderr();
    let mut out = stderr.lock();
    if format == MessageFormat::Short {
        for diagnostic in diagnostics {
            let _ = writeln!(out, "{}", diagnostic.short(sources));
        }
        return;
    }

    let use_color = color.should_color();
    let render_options = loom_diagnostics::render::RenderOptions { color: use_color };

    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.severity == loom_diagnostics::Severity::Warning)
        .count();

    for diagnostic in diagnostics {
        let rendered = loom_diagnostics::render::render(diagnostic, sources, &render_options);
        let _ = write!(out, "{rendered}");
        let _ = writeln!(out);
    }

    if let Some(summary) = summary_line(path, errors, warnings, use_color) {
        let _ = writeln!(out, "{summary}");
    }
}

fn print_diagnostics_for_session(session: &Session, format: MessageFormat, color: ColorChoice) {
    for (_, path, parse) in session.files() {
        let diagnostics = parse.diagnostics();
        if !diagnostics.is_empty() {
            print_diagnostics_for_file(diagnostics, session.sources(), path, format, color);
        }
    }
}

/// `loom check`: reports every error the implemented phases can find. Each file is
/// checked as the root of its own program.
#[allow(clippy::similar_names, reason = "names mirror the MIR terms")]
fn check(args: &[String]) -> ExitCode {
    let (options, paths) = split_options(args);
    let mut dump_cst = false;
    let mut dump_hir = false;
    let mut dump_mir = false;
    let mut dump_core = false;
    let mut format = MessageFormat::Rendered;
    let mut color = ColorChoice::Auto;
    for option in options {
        match option {
            "--dump=cst" => dump_cst = true,
            "--dump=hir" => dump_hir = true,
            "--dump=mir" => dump_mir = true,
            "--dump=mir-all" => {
                dump_mir = true;
                dump_core = true;
            }
            "--message-format=short" => format = MessageFormat::Short,
            "--message-format=rendered" | "--message-format=human" => {
                format = MessageFormat::Rendered;
            }
            "--color=always" => color = ColorChoice::Always,
            "--color=never" => color = ColorChoice::Never,
            "--color=auto" => color = ColorChoice::Auto,
            other => return usage_error(&format!("check: unknown option `{other}`")),
        }
    }
    if paths.is_empty() {
        return usage_error("check: expected at least one path");
    }
    let mut session = match load(&paths) {
        Ok(session) => session,
        Err(code) => return code,
    };
    let files: Vec<(loom_span::FileId, PathBuf)> = session
        .files()
        .map(|(id, path, _)| (id, path.to_owned()))
        .collect();
    let mut failed = false;
    for (file, path) in files {
        if dump_cst && let Some(parse) = session.parse(file) {
            println!("# {}", path.display());
            print!("{}", parse.syntax().debug_dump());
        }
        let analysis = session.analyze(file);
        if dump_hir {
            for body in &analysis.hir.bodies {
                if analysis.hir.package_of(body.owner) == loom_hir::Package::Main {
                    println!("# {}", analysis.hir.path_string(body.owner));
                    print!("{}", loom_hir::dump_body(body));
                }
            }
        }
        if dump_mir && let Some(mir) = &analysis.mir {
            for body in &mir.bodies {
                if dump_core || analysis.hir.package_of(body.owner) == loom_hir::Package::Main {
                    print!(
                        "{}",
                        loom_mir::dump_body(
                            &analysis.hir,
                            &analysis.checked.items,
                            &analysis.checked.interner,
                            body
                        )
                    );
                }
            }
        }
        print_diagnostics_for_file(
            &analysis.diagnostics,
            session.sources(),
            &path,
            format,
            color,
        );
        failed |= analysis.has_errors();
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Options of `loom run` and `loom test`.
#[derive(Clone, Copy)]
struct RunOptions {
    tests: bool,
    engine: Engine,
    verbose: bool,
    format: MessageFormat,
    color: ColorChoice,
}

/// How `loom run` executes a program.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Engine {
    /// Native code if the backend supports the program, otherwise the interpreter.
    Auto,
    /// The reference interpreter (`--interp`).
    Interp,
    /// Native code only (`--native`); unsupported programs are an error.
    Native,
}

/// `loom run [--interp|--native] [--verbose] <file> [-- args]` and `loom test <file>`:
/// checks the program, then runs `main` natively with the JIT (ADR 0021 §1) or in the
/// interpreter, or runs the `@[test]` functions (ADR 0019 §8–§9).
fn run(args: &[String], tests: bool) -> ExitCode {
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let (options, paths) = split_options(&args[..split]);
    let mut engine = if tests { Engine::Interp } else { Engine::Auto };
    let mut verbose = false;
    let mut format = MessageFormat::Rendered;
    let mut color = ColorChoice::Auto;
    for option in options {
        match option {
            "--interp" => engine = Engine::Interp,
            "--native" if !tests => engine = Engine::Native,
            "--verbose" => verbose = true,
            "--message-format=short" => format = MessageFormat::Short,
            "--message-format=rendered" | "--message-format=human" => {
                format = MessageFormat::Rendered;
            }
            "--color=always" => color = ColorChoice::Always,
            "--color=never" => color = ColorChoice::Never,
            "--color=auto" => color = ColorChoice::Auto,
            other => return usage_error(&format!("unknown option `{other}`")),
        }
    }
    let [path] = paths.as_slice() else {
        return usage_error(if tests {
            "test: expected one file"
        } else {
            "run: expected one file"
        });
    };
    let path = path.clone();
    // Deep recursion in the program needs a large native stack.
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            let engine = RunOptions {
                tests,
                engine,
                verbose,
                format,
                color,
            };
            run_on_this_thread(&path, engine)
        });
    if let Ok(Ok(code)) = worker.map(std::thread::JoinHandle::join) {
        ExitCode::from(code)
    } else {
        eprintln!("loom: internal error while running the program");
        ExitCode::from(USAGE)
    }
}

/// Loads and checks one file, printing diagnostics; `None` if it has errors.
fn analyze_file(
    path: &std::path::Path,
    format: MessageFormat,
    color: ColorChoice,
) -> Option<(Session, loom_driver::Analysis)> {
    let mut session = Session::new();
    let file = match session.load(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("loom: {error}");
            return None;
        }
    };
    let analysis = session.analyze(file);
    print_diagnostics_for_file(
        &analysis.diagnostics,
        session.sources(),
        path,
        format,
        color,
    );
    if analysis.has_errors() {
        return None;
    }
    Some((session, analysis))
}

/// `loom build [-o <output>] <file>`: compiles a program to a native executable with the
/// Cranelift backend and the system linker (ADR 0021 §1).
fn build(args: &[String]) -> ExitCode {
    let mut output: Option<PathBuf> = None;
    let mut inputs = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => match iter.next() {
                Some(o) => output = Some(PathBuf::from(o)),
                None => return usage_error("build: `-o` needs a file name"),
            },
            other if other.starts_with('-') => {
                return usage_error(&format!("build: unknown option `{other}`"));
            }
            other => inputs.push(PathBuf::from(other)),
        }
    }
    let [path] = inputs.as_slice() else {
        return usage_error("build: expected one file");
    };
    let Some((session, analysis)) = analyze_file(path, MessageFormat::Rendered, ColorChoice::Auto)
    else {
        return ExitCode::from(1);
    };
    let Some(main) = analysis.main() else {
        eprintln!(
            "{}: error[E0211]: a program needs a `main` function",
            path.display()
        );
        return ExitCode::from(1);
    };
    let Some(mir) = analysis.mir.as_ref() else {
        return ExitCode::from(1);
    };
    let program = loom_codegen_cranelift::Program {
        hir: &analysis.hir,
        checked: &analysis.checked,
        mir,
        sources: session.sources(),
    };
    let stem = path
        .file_stem()
        .map_or_else(|| "main".to_owned(), |s| s.to_string_lossy().into_owned());
    let output = output.unwrap_or_else(|| {
        PathBuf::from(if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem.clone()
        })
    });
    let object = match loom_codegen_cranelift::build_object(program, main, &stem) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("loom: {error}");
            return ExitCode::from(1);
        }
    };
    match link::link(&object, &output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("loom: {error}");
            ExitCode::from(1)
        }
    }
}

fn run_on_this_thread(path: &std::path::Path, options: RunOptions) -> u8 {
    let RunOptions {
        tests,
        engine,
        verbose,
        format,
        color,
    } = options;
    let Some((session, analysis)) = analyze_file(path, format, color) else {
        return 1;
    };
    if !tests {
        let Some(main) = analysis.main() else {
            eprintln!(
                "{}: error[E0211]: a program needs a `main` function",
                path.display()
            );
            return 1;
        };
        if engine != Engine::Interp
            && let Some(mir) = analysis.mir.as_ref()
        {
            let program = loom_codegen_cranelift::Program {
                hir: &analysis.hir,
                checked: &analysis.checked,
                mir,
                sources: session.sources(),
            };
            match loom_codegen_cranelift::run_jit(program, main) {
                Ok(status) => return u8::try_from(status).unwrap_or(1),
                Err(error) if engine == Engine::Native => {
                    eprintln!("loom: {error}");
                    return 1;
                }
                Err(error) => {
                    if verbose {
                        eprintln!("loom: running in the interpreter: {error}");
                    }
                }
            }
        }
    }
    // Tasks of the interpreted program hold the interpreter for the rest of the run, so
    // it lives as long as the process (ADR 0022 §9).
    let session: &'static Session = Box::leak(Box::new(session));
    let analysis: &'static loom_driver::Analysis = Box::leak(Box::new(analysis));
    let out = Box::new(std::io::BufWriter::new(std::io::stdout().lock()));
    let mut interp =
        loom_interp::Interp::new(&analysis.hir, &analysis.checked, session.sources(), out);
    if let Some(mir) = analysis.mir.as_ref() {
        interp = interp.with_mir(mir);
    }
    let interp: &'static loom_interp::Interp<'static> = Box::leak(Box::new(interp));
    if tests {
        let passed = interp.run_tests_scheduled(analysis.root);
        if let Some(message) = interp.take_task_panic() {
            eprintln!("{message}");
            return 101;
        }
        return if passed { 0 } else { 101 };
    }
    let Some(main) = analysis.main() else {
        return 1;
    };
    let outcome = interp.run_main_scheduled(main);
    interp.flush();
    if let Some(message) = interp.take_task_panic() {
        eprintln!("{message}");
        return 101;
    }
    match &outcome {
        loom_interp::Outcome::Error(message) | loom_interp::Outcome::Panicked(message) => {
            eprintln!("{message}");
        }
        loom_interp::Outcome::Finished => {}
    }
    outcome.exit_code()
}

/// `loom fmt`: formats files in place, or with `--check` reports files that would change.
fn fmt(args: &[String]) -> ExitCode {
    let (options, mut paths) = split_options(args);
    let mut check_only = false;
    let mut format = MessageFormat::Rendered;
    let mut color = ColorChoice::Auto;
    for option in options {
        match option {
            "--check" => check_only = true,
            "--message-format=short" => format = MessageFormat::Short,
            "--message-format=rendered" | "--message-format=human" => {
                format = MessageFormat::Rendered;
            }
            "--color=always" => color = ColorChoice::Always,
            "--color=never" => color = ColorChoice::Never,
            "--color=auto" => color = ColorChoice::Auto,
            other => return usage_error(&format!("fmt: unknown option `{other}`")),
        }
    }
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let session = match load(&paths) {
        Ok(session) => session,
        Err(code) => return code,
    };
    if session.has_errors() {
        print_diagnostics_for_session(&session, format, color);
        eprintln!("loom fmt: not formatting files with errors");
        return ExitCode::FAILURE;
    }
    let mut changed = 0;
    for (file, path, parse) in session.files() {
        let original = session.sources().file(file).text();
        let formatted = loom_fmt::format_tree(&parse.syntax());
        if formatted == original {
            continue;
        }
        changed += 1;
        if check_only {
            println!("would reformat {}", path.display());
        } else if let Err(error) = std::fs::write(path, formatted) {
            eprintln!("loom fmt: {}: {error}", path.display());
            return ExitCode::FAILURE;
        } else {
            println!("formatted {}", path.display());
        }
    }
    if check_only && changed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `loom explain CODE`: prints what an error code means.
fn explain(args: &[String]) -> ExitCode {
    let [code] = args else {
        return usage_error("explain: expected one error code, such as `loom explain E0007`");
    };
    if let Some(text) = explain::explanation(code) {
        print!("{text}");
        ExitCode::SUCCESS
    } else {
        eprintln!("loom explain: `{code}` is not a Loom error code");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::COMMANDS;

    #[test]
    fn command_names_are_unique() {
        let mut names: Vec<_> = COMMANDS.iter().map(|command| command.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len());
    }

    #[test]
    fn every_command_is_scheduled_in_a_roadmap_phase() {
        assert!(
            COMMANDS
                .iter()
                .all(|command| (1..=7).contains(&command.phase))
        );
    }
}
