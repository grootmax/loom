//! The `loom` command-line tool.
//!
//! Status: Phase 0 stub. Only `--version` and `help` work; every other command
//! reports which roadmap phase will implement it and exits with status 2.

use std::process::ExitCode;

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
    cmd("check", "Type-check without producing a binary", 2),
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
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        print_help();
        return ExitCode::SUCCESS;
    };

    match first.as_str() {
        "-V" | "--version" | "version" => {
            println!("loom {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "-h" | "--help" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
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
            ExitCode::from(2)
        }
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
