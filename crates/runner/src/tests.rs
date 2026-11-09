#![allow(clippy::arc_with_non_send_sync)]

use crate::exec_program;
use crate::{execute, runner::ExecProgramArgs};
use anyhow::Result;
use kcl_ast::ast::{Module, Program};
use kcl_config::settings::load_file;
use kcl_parser::ParseSession;
use kcl_parser::load_program;
use kcl_utils::path::PathPrefix;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;
use std::{
    collections::HashMap,
    fs::{self, File},
};
use uuid::Uuid;
use walkdir::WalkDir;

const TEST_CASES: &[&str; 5] = &[
    "init_check_order_0",
    "init_check_order_1",
    "normal_2",
    "type_annotation_not_full_2",
    "multi_vars_0",
];

fn exec_data_path() -> String {
    Path::new("src").join("exec_data").display().to_string()
}

fn exec_err_data_path() -> String {
    Path::new("src").join("exec_err_data").display().to_string()
}

fn custom_manifests_data_path() -> String {
    Path::new("src")
        .join("custom_manifests_data")
        .display()
        .to_string()
}

fn exec_prog_args_test_case() -> Vec<String> {
    vec![
        Path::new("exec_prog_args")
            .join("default.json")
            .display()
            .to_string(),
    ]
}

fn settings_file_test_case() -> Vec<(String, String)> {
    vec![(
        Path::new("settings_file")
            .join("settings.yaml")
            .display()
            .to_string(),
        Path::new("settings_file")
            .join("settings.json")
            .display()
            .to_string(),
    )]
}

const EXPECTED_JSON_FILE_NAME: &str = "stdout.golden.json";

fn test_case_path() -> String {
    Path::new("src").join("test_datas").display().to_string()
}

const KCL_FILE_NAME: &str = "main.k";
const MAIN_PKG_NAME: &str = "__main__";

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SimplePanicInfo {
    line: i32,
    col: i32,
    message: String,
}

/// Load test kcl file to ast.Program
fn load_test_program(filename: String) -> Program {
    let module = kcl_parser::parse_file_force_errors(&filename, None).unwrap();
    construct_program(module)
}

/// Construct ast.Program by ast.Module and default configuration.
/// Default configuration:
///     module.pkg = "__main__"
///     Program.root = "__main__"
fn construct_program(module: Module) -> Program {
    let mut pkgs_ast = HashMap::new();
    pkgs_ast.insert(MAIN_PKG_NAME.to_string(), vec![module.filename.clone()]);
    let mut modules = HashMap::new();
    modules.insert(module.filename.clone(), Arc::new(RwLock::new(module)));
    Program {
        root: MAIN_PKG_NAME.to_string(),
        pkgs: pkgs_ast,
        modules,
        pkgs_not_imported: HashMap::new(),
        modules_not_imported: HashMap::new(),
    }
}

/// Load the expect result from stdout.golden.json
fn load_expect_file(filename: String) -> String {
    let f = File::open(filename).unwrap();
    let v: serde_json::Value = serde_json::from_reader(f).unwrap();
    v.to_string()
}

/// Format str by json str
fn format_str_by_json(str: String) -> String {
    let v: serde_json::Value = serde_json::from_str(&str).unwrap();
    v.to_string()
}

fn execute_for_test(kcl_path: &String) -> String {
    let args = ExecProgramArgs::default();
    // Parse kcl file
    let program = load_test_program(kcl_path.to_string());
    // Generate libs, link libs and execute.
    execute(Arc::new(ParseSession::default()), program, &args)
        .unwrap()
        .json_result
}

fn test_kcl_runner_execute() {
    for case in TEST_CASES {
        let kcl_path = &Path::new(&test_case_path())
            .join(case)
            .join(KCL_FILE_NAME)
            .display()
            .to_string();
        let expected_path = &Path::new(&test_case_path())
            .join(case)
            .join(EXPECTED_JSON_FILE_NAME)
            .display()
            .to_string();
        let result = execute_for_test(kcl_path);
        let expected_result = load_expect_file(expected_path.to_string());
        assert_eq!(expected_result, format_str_by_json(result));
    }
}

#[test]
fn test_to_json_program_arg() {
    for case in exec_prog_args_test_case() {
        let test_case_json_file = &Path::new(&test_case_path())
            .join(case)
            .display()
            .to_string();
        let expected_json_str = fs::read_to_string(test_case_json_file).unwrap();
        let exec_prog_args = ExecProgramArgs::default();
        assert_eq!(expected_json_str.trim(), exec_prog_args.to_json().trim());
    }
}

#[test]
fn test_from_str_program_arg() {
    for case in exec_prog_args_test_case() {
        let test_case_json_file = &Path::new(&test_case_path())
            .join(case)
            .display()
            .to_string();
        let expected_json_str = fs::read_to_string(test_case_json_file).unwrap();
        let exec_prog_args = ExecProgramArgs::from_json(&expected_json_str);
        assert_eq!(expected_json_str.trim(), exec_prog_args.to_json().trim());
    }
}

#[test]
fn test_from_setting_file_program_arg() {
    for (case_yaml, case_json) in settings_file_test_case() {
        let test_case_yaml_file = &Path::new(&test_case_path())
            .join(case_yaml)
            .display()
            .to_string();
        let settings_file = load_file(test_case_yaml_file).unwrap();

        let test_case_json_file = &Path::new(&test_case_path())
            .join(case_json)
            .display()
            .to_string();
        let expected_json_str = fs::read_to_string(test_case_json_file).unwrap();

        let exec_prog_args = ExecProgramArgs::try_from(settings_file).unwrap();
        assert_eq!(expected_json_str.trim(), exec_prog_args.to_json().trim());
    }
}

fn test_exec_file() {
    let result = std::panic::catch_unwind(|| {
        for file in get_files(exec_data_path(), false, true, ".k") {
            exec(&file).unwrap();
            println!("{} - PASS", file);
        }
    });
    assert!(result.is_ok());
}

fn test_custom_manifests_output() {
    exec_with_result_at(&custom_manifests_data_path());
}

fn test_exec_with_err_result() {
    exec_with_err_result_at(&exec_err_data_path());
}

fn clean_dir(path: String) {
    if fs::remove_dir_all(path).is_ok() {}
}

#[test]
fn test_exec() {
    clean_dir(
        Path::new(".")
            .join("src")
            .join("exec_data")
            .join(".kcl")
            .display()
            .to_string(),
    );

    clean_dir(
        Path::new(".")
            .join("src")
            .join("exec_err_data")
            .join(".kcl")
            .display()
            .to_string(),
    );

    test_exec_file();
    println!("test_exec_file - PASS");

    test_kcl_runner_execute();
    println!("test_kcl_runner_execute - PASS");

    test_custom_manifests_output();
    println!("test_custom_manifests_output - PASS");

    test_exec_with_err_result();
    println!("test_exec_with_err_result - PASS");

    test_indent_error();
    println!("test_indent_error - PASS");

    test_compile_with_file_pattern();
    println!("test_compile_with_file_pattern - PASS");

    test_uuid();
    println!("test_uuid - PASS");
}

fn test_indent_error() {
    let test_path = PathBuf::from("./src/test_indent_error");
    let kcl_files = get_files(test_path.clone(), false, true, ".k");
    let output_files = get_files(test_path, false, true, ".stderr");

    for (kcl_file, err_file) in kcl_files.iter().zip(&output_files) {
        let mut args = ExecProgramArgs::default();
        args.k_filename_list.push(kcl_file.to_string());
        let res = exec_program(Arc::new(ParseSession::default()), &args);
        assert!(res.is_err());
        if let Err(err_msg) = res {
            let expect_err = fs::read_to_string(err_file).expect("Failed to read file");
            assert!(err_msg.to_string().contains(&expect_err));
        }
    }
}

fn exec(file: &str) -> Result<String, String> {
    let mut args = ExecProgramArgs::default();
    args.k_filename_list.push(file.to_string());
    let opts = args.get_load_program_options();
    let sess = Arc::new(ParseSession::default());
    // Load AST program
    let program = load_program(sess.clone(), &[file], Some(opts), None)
        .unwrap()
        .program;
    // Resolve ATS, generate libs, link libs and execute.
    match execute(sess, program, &args) {
        Ok(result) => {
            if result.err_message.is_empty() {
                Ok(result.json_result)
            } else {
                Err(result.err_message)
            }
        }
        Err(err) => Err(err.to_string()),
    }
}

/// Run all kcl files at path and compare the exec result with the expect output.
fn exec_with_result_at(path: &str) {
    let kcl_files = get_files(path, false, true, ".k");
    let output_files = get_files(path, false, true, ".stdout.golden");
    for (kcl_file, output_file) in kcl_files.iter().zip(&output_files) {
        let mut args = ExecProgramArgs::default();
        args.k_filename_list.push(kcl_file.to_string());
        let result = exec_program(Arc::new(ParseSession::default()), &args).unwrap();

        #[cfg(not(target_os = "windows"))]
        let newline = "\n";
        #[cfg(target_os = "windows")]
        let newline = "\r\n";

        let expected_str = std::fs::read_to_string(output_file).unwrap();
        let expected = expected_str
            .strip_suffix(newline)
            .unwrap_or(&expected_str)
            .to_string();

        #[cfg(target_os = "windows")]
        let expected = expected.replace("\r\n", "\n");

        assert_eq!(
            result.yaml_result, expected,
            "test case {} {} failed",
            path, kcl_file
        );
    }
}

/// Run all kcl files at path and compare the exec error result with the expect error output.
fn exec_with_err_result_at(path: &str) {
    let kcl_files = get_files(path, false, true, ".k");
    let output_files = get_files(path, false, true, ".stderr.json");

    let result = crate::runner::catch_unwind_silent(|| {
        for (kcl_file, _) in kcl_files.iter().zip(&output_files) {
            let mut args = ExecProgramArgs::default();
            args.k_filename_list.push(kcl_file.to_string());
            let result = exec_program(Arc::new(ParseSession::default()), &args);
            if let Ok(result) = result {
                assert!(!result.err_message.is_empty(), "{}", result.err_message);
            } else {
                assert!(result.is_err());
            }
        }
    });
    assert!(result.is_ok());
}

/// Get kcl files from path.
fn get_files<P: AsRef<Path>>(
    path: P,
    recursively: bool,
    sorted: bool,
    suffix: &str,
) -> Vec<String> {
    let mut files = vec![];
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            let file = path.to_str().unwrap();
            if file.ends_with(suffix) && (recursively || entry.depth() == 1) {
                files.push(file.to_string())
            }
        }
    }
    if sorted {
        files.sort();
    }
    files
}

fn test_compile_with_file_pattern() {
    let test_path = PathBuf::from("./src/test_file_pattern/**/main.k");
    let mut args = ExecProgramArgs::default();
    args.k_filename_list.push(test_path.display().to_string());
    let res = exec_program(Arc::new(ParseSession::default()), &args);
    assert!(res.is_ok());
    assert_eq!(
        res.as_ref().unwrap().yaml_result,
        "k3: Hello World!\nk1: Hello World!\nk2: Hello World!"
    );
    assert_eq!(
        res.as_ref().unwrap().json_result,
        "{\"k3\": \"Hello World!\", \"k1\": \"Hello World!\", \"k2\": \"Hello World!\"}"
    );
}

fn test_uuid() {
    let res = exec(
        &PathBuf::from(".")
            .join("src")
            .join("test_uuid")
            .join("main.k")
            .canonicalize()
            .unwrap()
            .display()
            .to_string(),
    );

    let v: Value = serde_json::from_str(res.clone().unwrap().as_str()).unwrap();
    assert!(v["a"].as_str().is_some());
    if let Some(uuid_str) = v["a"].as_str() {
        assert!(Uuid::parse_str(uuid_str).is_ok());
    }
}

#[test]
fn test_compile_with_symbolic_link() {
    let main_test_path = PathBuf::from("./src/test_symbolic_link/test_pkg/bbb/main.k");
    let mut args = ExecProgramArgs::default();
    args.k_filename_list
        .push(main_test_path.display().to_string());
    let res = exec_program(Arc::new(ParseSession::default()), &args);
    assert!(res.is_ok());
    assert_eq!(
        res.as_ref().unwrap().yaml_result,
        "The_first_kcl_program: Hello World!\nb: 1"
    );
    assert_eq!(
        res.as_ref().unwrap().json_result,
        "{\"The_first_kcl_program\": \"Hello World!\", \"b\": 1}"
    );
}

#[test]
fn test_kcl_issue_1799() {
    let main_test_path = PathBuf::from("./src/test_issues/github.com/kcl-lang/kcl/1799/main.k");
    let mut args = ExecProgramArgs::default();
    args.k_filename_list
        .push(main_test_path.display().to_string());
    args.work_dir = Some(".".to_string());
    let res = exec_program(Arc::new(ParseSession::default()), &args);
    assert!(res.is_ok());
    assert_eq!(
        res.as_ref().unwrap().yaml_result,
        format!(
            "a: {}",
            main_test_path
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .adjust_canonicalization()
        )
    );
}

/// Regression test for issue #2216: a compile error must reach callers as a
/// [`RenderedError`] carrying the structured diagnostics, so that
/// machine-readable renderers (arcanist / SARIF) can report real source
/// positions instead of re-wrapping the rendered text.
#[test]
fn test_compile_error_carries_structured_diagnostics() {
    let mut args = ExecProgramArgs::default();
    args.k_filename_list.push(
        PathBuf::from("./src/test_issues/github.com/kcl-lang/kcl/2216/compile_error.k")
            .display()
            .to_string(),
    );
    args.work_dir = Some(".".to_string());
    let err = exec_program(Arc::new(ParseSession::default()), &args)
        .expect_err("a type error must fail compilation");

    let rendered = err
        .downcast_ref::<kcl_error::RenderedError>()
        .expect("compile errors must carry their diagnostics");
    // `RenderedError` must display exactly the rendered text so the human
    // readable output is unchanged.
    assert_eq!(rendered.to_string(), rendered.message);
    assert!(
        rendered.message.contains("expected int, got str"),
        "unexpected message: {}",
        rendered.message
    );

    let diag = rendered
        .diagnostics
        .first()
        .expect("at least one diagnostic");
    let pos = &diag.messages[0].range.0;
    assert!(
        pos.filename.ends_with("compile_error.k"),
        "compile diagnostics must name the source file: {pos:?}"
    );
    assert_eq!(
        pos.line, 1,
        "compile diagnostics must carry a line: {pos:?}"
    );
    assert!(
        pos.column.is_some(),
        "compile diagnostics must carry a column"
    );
}

/// An evaluation error must leave the diagnostics on the result so the CLI
/// and the service can re-render them with their source position intact.
#[test]
fn test_eval_error_reports_diagnostics_on_result() {
    let main_test_path = PathBuf::from("./src/test_issues/github.com/kcl-lang/kcl/2216/main.k");
    let mut args = ExecProgramArgs::default();
    args.k_filename_list
        .push(main_test_path.display().to_string());
    args.work_dir = Some(".".to_string());
    let res = exec_program(Arc::new(ParseSession::default()), &args)
        .expect("a runtime failure is reported through the result, not an Err");

    assert!(!res.err_message.is_empty());
    assert_eq!(
        res.diagnostics.len(),
        1,
        "expected exactly one runtime diagnostic"
    );
    let diag = &res.diagnostics[0];
    let pos = &diag.messages[0].range.0;
    assert!(
        pos.filename.ends_with("main.k"),
        "runtime diagnostics must name the source file: {pos:?}"
    );
    assert_eq!(
        pos.line, 2,
        "runtime diagnostics must carry a line: {pos:?}"
    );
}

/// Regression test for issue #2223: evaluations running concurrently in one
/// process must each report their own runtime error message.
#[test]
fn test_concurrent_eval_errors_keep_message() {
    let main_test_path = PathBuf::from("./src/test_issues/github.com/kcl-lang/kcl/2223/main.k");
    for _ in 0..20 {
        // Each evaluation runs on a fresh thread so that no message can
        // carry over from an earlier evaluation on the same thread.
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let main_test_path = main_test_path.clone();
                std::thread::spawn(move || {
                    let mut args = ExecProgramArgs::default();
                    args.k_filename_list
                        .push(main_test_path.display().to_string());
                    args.work_dir = Some(".".to_string());
                    let res = exec_program(Arc::new(ParseSession::default()), &args).unwrap();
                    res.diagnostics[0].messages[0].message.clone()
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), "items must be empty");
        }
    }
}

/// A host that replaces the panic hook after KCL installed its own must not
/// lose runtime error messages.
#[test]
fn test_eval_error_keeps_message_after_hook_replaced() {
    let mut args = ExecProgramArgs::default();
    args.k_filename_list
        .push("./src/test_issues/github.com/kcl-lang/kcl/2223/main.k".to_string());
    args.work_dir = Some(".".to_string());
    // Install KCL's hook, then replace it as a host would.
    crate::runner::catch_unwind_silent(|| ()).unwrap();
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = exec_program(Arc::new(ParseSession::default()), &args);
    std::panic::set_hook(prev_hook);
    assert_eq!(
        res.unwrap().diagnostics[0].messages[0].message,
        "items must be empty"
    );
}

#[derive(Clone, Default)]
struct TimingWriter(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for TimingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn traced_exec(code: &str, compile_only: bool) -> (Result<crate::ExecProgramResult>, Vec<Value>) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("timing.k");
    fs::write(&file, code).unwrap();
    let args = ExecProgramArgs {
        k_filename_list: vec![file.display().to_string()],
        compile_only,
        ..Default::default()
    };
    let writer = TimingWriter::default();
    let output = writer.0.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let result = tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("deployment.render", revision_id = "timing-revision");
        let _entered = span.enter();
        exec_program(Arc::new(ParseSession::default()), &args)
    });
    let logs = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    assert!(!logs.contains("timing-payload-must-not-appear"));
    let events = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (result, events)
}

#[test]
fn test_phase_timings_preserve_output_and_revision_correlation() {
    let (result, events) = traced_exec("result = \"timing-payload-must-not-appear\"", false);
    assert_eq!(
        serde_json::from_str::<Value>(&result.unwrap().json_result).unwrap(),
        serde_json::json!({"result": "timing-payload-must-not-appear"})
    );
    for stage in [
        "kcl_load_program",
        "kcl_apply_overrides",
        "kcl_resolve",
        "kcl_diagnostics",
        "kcl_initialize",
        "kcl_evaluate",
        "kcl_plan_output",
        "kcl_evaluate_and_plan",
        "kcl_result",
        "kcl_gc",
        "kcl_drop_program",
    ] {
        let event = events
            .iter()
            .find(|event| event["fields"]["stage"] == stage)
            .unwrap_or_else(|| panic!("missing timing: {stage}"));
        assert!(event["fields"]["elapsed_ms"].as_f64().unwrap() >= 0.0);
        assert!(
            event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|span| { span["revision_id"] == "timing-revision" })
        );
    }
}

#[test]
fn test_phase_timings_report_evaluation_failure_without_source() {
    let (result, events) = traced_exec("assert False, \"timing-payload-must-not-appear\"", false);
    assert!(!result.unwrap().err_message.is_empty());
    let event = events
        .iter()
        .find(|event| event["fields"]["stage"] == "kcl_evaluate_and_plan")
        .unwrap();
    assert_eq!(event["fields"]["success"], false);
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["stage"] == "kcl_gc")
    );
}

#[test]
fn test_phase_timings_compile_only_does_not_evaluate() {
    let (result, events) = traced_exec("result = 1", true);
    assert!(result.unwrap().err_message.is_empty());
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["stage"] == "kcl_resolve")
    );
    assert!(
        !events
            .iter()
            .any(|event| event["fields"]["stage"] == "kcl_evaluate")
    );
}

fn cache_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("kcl.mod"), "").unwrap();
    fs::create_dir(dir.path().join("pkg")).unwrap();
    fs::write(
        dir.path().join("pkg/model.k"),
        "schema Record:\n    value: int\n",
    )
    .unwrap();
    fs::write(dir.path().join("main.k"), "import pkg\nvalue = option(\"input\")\nassert value >= 0, \"negative ${value}\"\nresult = pkg.Record {value = value}\n").unwrap();
    dir
}

fn cache_args(root: &Path, value: i32) -> ExecProgramArgs {
    ExecProgramArgs {
        work_dir: Some(root.display().to_string()),
        k_filename_list: vec![root.join("main.k").display().to_string()],
        args: vec![kcl_ast::ast::Argument {
            name: "input".to_string(),
            value: value.to_string(),
        }],
        ..Default::default()
    }
}

#[test]
fn test_cache_isolates_inputs_overrides_and_output_formats() {
    let dir = cache_fixture();
    let cache = crate::ProgramCache::default();
    for value in 0..6 {
        let mut args = cache_args(dir.path(), value);
        if value == 2 {
            args.overrides = vec!["result.value=42".to_string()];
        }
        if value == 4 {
            args.format = Some("json".to_string());
        }
        let baseline = exec_program(Default::default(), &args).unwrap();
        let cached = crate::exec_program_with_cache(Default::default(), &args, &cache).unwrap();
        assert_eq!(cached.err_message, baseline.err_message);
        assert_eq!(cached.json_result, baseline.json_result);
        assert_eq!(cached.yaml_result, baseline.yaml_result);
    }
}

#[test]
fn test_cache_preserves_repeated_import_errors_and_explicit_invalidation() {
    let dir = cache_fixture();
    let cache = crate::ProgramCache::default();
    let args = cache_args(dir.path(), 1);
    fs::write(
        dir.path().join("pkg/model.k"),
        "schema Record\n    value = (\n",
    )
    .unwrap();
    for _ in 0..2 {
        assert!(crate::exec_program_with_cache(Default::default(), &args, &cache).is_err());
    }
    fs::write(
        dir.path().join("pkg/model.k"),
        "schema Record:\n    value: int\n",
    )
    .unwrap();
    cache.clear().unwrap();
    assert!(
        crate::exec_program_with_cache(Default::default(), &args, &cache)
            .unwrap()
            .err_message
            .is_empty()
    );
    fs::write(
        dir.path().join("pkg/model.k"),
        "schema Record:\n    value: int\n    generation: int = 2\n",
    )
    .unwrap();
    fs::write(dir.path().join("pkg/extra.k"), "additional = True\n").unwrap();
    cache.clear().unwrap();
    let cached = crate::exec_program_with_cache(Default::default(), &args, &cache).unwrap();
    let baseline = exec_program(Default::default(), &args).unwrap();
    assert_eq!(cached.json_result, baseline.json_result);
    assert!(cached.json_result.contains("generation"));
}

#[test]
fn test_cache_concurrent_reused_workers_keep_distinct_values_and_errors() {
    let dir = cache_fixture();
    let cache = Arc::new(crate::ProgramCache::default());
    std::thread::scope(|scope| {
        for worker in 0..2 {
            let cache = cache.clone();
            let root = dir.path();
            scope.spawn(move || {
                for iteration in 0..20 {
                    let value = 1000 * (worker + 1) + iteration;
                    let args = cache_args(root, if iteration % 3 == 0 { -value } else { value });
                    let result =
                        crate::exec_program_with_cache(Default::default(), &args, &cache).unwrap();
                    if iteration % 3 == 0 {
                        assert!(
                            result.err_message.contains(&format!("negative -{value}")),
                            "{}",
                            result.err_message
                        );
                    } else {
                        assert!(result.err_message.is_empty(), "{}", result.err_message);
                        let output: Value = serde_json::from_str(&result.json_result).unwrap();
                        assert_eq!(output["result"]["value"], value);
                    }
                }
            });
        }
    });
}

#[test]
fn test_cache_changes_source_namespace_and_rejects_side_effect_modes() {
    let first = cache_fixture();
    let second = cache_fixture();
    fs::write(
        second.path().join("pkg/model.k"),
        "schema Record:\n    value: int\n    second: bool = True\n",
    )
    .unwrap();
    let cache = crate::ProgramCache::default();
    for root in [first.path(), second.path(), first.path()] {
        let args = cache_args(root, 1);
        let cached = crate::exec_program_with_cache(Default::default(), &args, &cache).unwrap();
        assert_eq!(
            cached.json_result,
            exec_program(Default::default(), &args).unwrap().json_result
        );
    }
    for field in 0..3 {
        let mut args = cache_args(first.path(), 1);
        match field {
            0 => args.plugin_agent = 1,
            1 => args.print_override_ast = true,
            _ => args.debug = 1,
        }
        assert!(crate::exec_program_with_cache(Default::default(), &args, &cache).is_err());
    }
}

#[test]
fn test_cache_keeps_imported_semantic_diagnostics_on_reuse() {
    let dir = cache_fixture();
    fs::write(
        dir.path().join("pkg/model.k"),
        "schema Record:\n    value: UndefinedType\n",
    )
    .unwrap();
    let args = cache_args(dir.path(), 1);
    let cache = crate::ProgramCache::default();
    let baseline = exec_program(Default::default(), &args)
        .unwrap_err()
        .to_string();
    for _ in 0..2 {
        let error = crate::exec_program_with_cache(Default::default(), &args, &cache)
            .unwrap_err()
            .to_string();
        assert_eq!(error, baseline);
    }
}

#[test]
fn test_cache_reuses_raw_modules_across_entry_and_import_contexts() {
    let dir = cache_fixture();
    let main = dir.path().join("main.k");
    let shared = dir.path().join("shared.k");
    fs::write(
        &shared,
        "import pkg\nresult = pkg.Record {value = option(\"input\")}\n",
    )
    .unwrap();
    fs::write(&main, "import shared\nresult = shared.result\n").unwrap();
    let cache = crate::ProgramCache::default();
    for path in [&shared, &main, &shared, &main] {
        let mut args = cache_args(dir.path(), 4);
        args.k_filename_list = vec![path.display().to_string()];
        let baseline = exec_program(Default::default(), &args).unwrap();
        let cached = crate::exec_program_with_cache(Default::default(), &args, &cache).unwrap();
        assert_eq!(cached.err_message, baseline.err_message);
        assert_eq!(cached.json_result, baseline.json_result);
    }
}
