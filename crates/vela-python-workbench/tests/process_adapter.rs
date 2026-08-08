use std::{path::PathBuf, time::Duration};

use vela_python_workbench::{
    HamelnbProcessAdapter, PythonExecutionError, PythonExecutionLimits, PythonExecutionRequest,
};

fn adapter_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-hamelnb.py")
}

#[test]
fn process_adapter_preserves_multiline_python_and_explicit_target() {
    let source = "values = [1, 2, 3]\nprint(sum(values))\n";
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", source).unwrap();
    let result = HamelnbProcessAdapter::new(adapter_path())
        .execute(&request)
        .expect("successful execution");

    assert_eq!(result.status(), "ok");
    assert_eq!(result.transport(), Some("websocket"));
    assert_eq!(result.value()["observed_code"], source);
    assert_eq!(result.value()["source_exposed_in_argv"], false);
    assert_eq!(result.value()["observed_port"], "8888");
    assert_eq!(result.value()["observed_path"], "scratch.ipynb");
}

#[test]
fn process_adapter_kills_a_direct_child_after_the_runtime_budget() {
    let limits = PythonExecutionLimits::new(Duration::from_millis(50), 1024).unwrap();
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", "__vela_test_sleep__")
        .unwrap()
        .with_limits(limits);

    let started = std::time::Instant::now();
    let error = HamelnbProcessAdapter::new(adapter_path())
        .execute(&request)
        .expect_err("sleeping adapter must time out");

    assert!(
        matches!(error, PythonExecutionError::TimedOut { .. }),
        "unexpected error: {error:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn successful_adapter_is_not_held_open_by_inherited_pipes() {
    // The fixture's descendant holds both output pipes for 250 ms, beyond this
    // direct-child runtime budget. The direct child still exits within budget.
    let limits = PythonExecutionLimits::new(Duration::from_millis(200), 1024).unwrap();
    let request =
        PythonExecutionRequest::new(8888, "scratch.ipynb", "__vela_test_inherited_pipe__")
            .unwrap()
            .with_limits(limits);

    let started = std::time::Instant::now();
    let result = HamelnbProcessAdapter::new(adapter_path())
        .execute(&request)
        .expect("an exited adapter must not wait for a descendant's inherited pipes");

    assert_eq!(result.status(), "ok");
    assert!(started.elapsed() < Duration::from_millis(250));
    // The fixture leaves a grandchild that sleeps 0.25 s. Wait for it to exit
    // so it does not outlive this test run.
    std::thread::sleep(Duration::from_millis(300));
}

#[test]
fn process_adapter_rejects_stdout_beyond_the_capture_budget() {
    let limits = PythonExecutionLimits::new(Duration::from_secs(1), 128).unwrap();
    let request =
        PythonExecutionRequest::new(8888, "scratch.ipynb", "__vela_test_stdout_overflow__")
            .unwrap()
            .with_limits(limits);

    let error = HamelnbProcessAdapter::new(adapter_path())
        .execute(&request)
        .expect_err("oversized stdout must fail");

    assert!(
        matches!(
            error,
            PythonExecutionError::OutputLimitExceeded {
                stream: "stdout",
                limit: 128
            }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn continuous_output_cannot_starve_the_runtime_budget() {
    let limits = PythonExecutionLimits::new(Duration::from_millis(50), 100 * 1024 * 1024).unwrap();
    let request =
        PythonExecutionRequest::new(8888, "scratch.ipynb", "__vela_test_continuous_output__")
            .unwrap()
            .with_limits(limits);

    let started = std::time::Instant::now();
    let error = HamelnbProcessAdapter::new(adapter_path())
        .execute(&request)
        .expect_err("continuous output must not starve the deadline");

    assert!(matches!(error, PythonExecutionError::TimedOut { .. }));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn process_adapter_rejects_stderr_beyond_the_capture_budget() {
    let limits = PythonExecutionLimits::new(Duration::from_secs(1), 128).unwrap();
    let request =
        PythonExecutionRequest::new(8888, "scratch.ipynb", "__vela_test_stderr_overflow__")
            .unwrap()
            .with_limits(limits);

    let error = HamelnbProcessAdapter::new(adapter_path())
        .execute(&request)
        .expect_err("oversized stderr must fail");

    assert!(matches!(
        error,
        PythonExecutionError::OutputLimitExceeded {
            stream: "stderr",
            limit: 128
        }
    ));
}

#[test]
fn execution_limits_reject_zero_budgets() {
    assert!(PythonExecutionLimits::new(Duration::ZERO, 1).is_err());
    assert!(PythonExecutionLimits::new(Duration::from_secs(1), 0).is_err());
}
