//! Linking native executables (ADR 0021 §1): the object file from the backend, the
//! runtime static library and the system libraries the Rust standard library needs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The file name of the runtime static library on this host.
fn runtime_lib_name() -> &'static str {
    if cfg!(windows) {
        "loom_runtime.lib"
    } else {
        "libloom_runtime.a"
    }
}

/// Finds the runtime static library: `LOOM_RUNTIME_LIB`, then next to the `loom`
/// executable, then in `../lib` relative to it.
fn runtime_lib() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("LOOM_RUNTIME_LIB") {
        return Ok(PathBuf::from(path));
    }
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot locate the loom executable: {e}"))?;
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let name = runtime_lib_name();
    for candidate in [dir.join(name), dir.join("..").join("lib").join(name)] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "cannot find the runtime library `{name}` next to the loom executable; set LOOM_RUNTIME_LIB"
    ))
}

/// System libraries the runtime (Rust's standard library) needs.
fn system_libs() -> &'static [&'static str] {
    if cfg!(target_os = "linux") {
        &[
            "-lgcc_s",
            "-lutil",
            "-lrt",
            "-lpthread",
            "-lm",
            "-ldl",
            "-lc",
        ]
    } else if cfg!(target_os = "macos") {
        &["-lSystem", "-lc", "-lm"]
    } else if cfg!(windows) {
        &[
            "kernel32.lib",
            "advapi32.lib",
            "ntdll.lib",
            "userenv.lib",
            "ws2_32.lib",
            "bcrypt.lib",
            "msvcrt.lib",
            "legacy_stdio_definitions.lib",
        ]
    } else {
        &["-lpthread", "-lm", "-lc"]
    }
}

/// Writes `object` to a temporary file and links it into the executable `output`.
pub fn link(object: &[u8], output: &Path) -> Result<(), String> {
    let runtime = runtime_lib()?;
    let dir = std::env::temp_dir().join(format!("loom-build-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let object_path = dir.join(if cfg!(windows) { "main.obj" } else { "main.o" });
    std::fs::write(&object_path, object)
        .map_err(|e| format!("cannot write {}: {e}", object_path.display()))?;
    let mut command = if cfg!(windows) {
        let mut c = Command::new("link.exe");
        c.arg("/NOLOGO")
            .arg(format!("/OUT:{}", output.display()))
            .arg(&object_path)
            .arg(&runtime);
        c
    } else {
        let linker = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
        let mut c = Command::new(linker);
        c.arg("-o").arg(output).arg(&object_path).arg(&runtime);
        c
    };
    command.args(system_libs());
    let result = command.output();
    let _ = std::fs::remove_dir_all(&dir);
    let out = result.map_err(|e| format!("cannot run the system linker: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "linking failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}
