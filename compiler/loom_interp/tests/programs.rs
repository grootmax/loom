//! Runs every program in `tests/programs/` in process and compares its output with
//! `<name>.stdout` and its exit status with `<name>.exit` (default 0).

use std::path::Path;

fn run(path: &Path) -> (String, u8) {
    let mut session = loom_driver::Session::new();
    let file = session.load(path).expect("program loads");
    let analysis = session.analyze(file);
    let errors: Vec<String> = analysis
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.short(session.sources()))
        .collect();
    assert!(errors.is_empty(), "{}: {errors:?}", path.display());
    let mut output = Vec::new();
    let code = {
        let interp = loom_interp::Interp::new(
            &analysis.hir,
            &analysis.checked,
            session.sources(),
            Box::new(&mut output),
        );
        let main = analysis.main().expect("program has main");
        interp.run_main(main).exit_code()
    };
    (String::from_utf8(output).expect("utf-8 output"), code)
}

#[test]
fn programs_produce_their_expected_output() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("programs");
    let mut entries: Vec<_> = std::fs::read_dir(&directory)
        .expect("programs directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "loom"))
        .collect();
    entries.sort();
    assert_ne!(entries.len(), 0);
    for path in entries {
        let expected =
            std::fs::read_to_string(path.with_extension("stdout")).expect("expected output");
        let expected_code: u8 = std::fs::read_to_string(path.with_extension("exit"))
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0);
        let (output, code) = run(&path);
        assert_eq!(output, expected, "{}", path.display());
        assert_eq!(code, expected_code, "{}", path.display());
    }
}
