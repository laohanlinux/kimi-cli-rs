use std::process::Command;

#[test]
fn default_mode_falls_back_gracefully_in_non_interactive_process() {
    let exe = env!("CARGO_BIN_EXE_kimi-cli-rs");

    let output = Command::new(exe)
        .output()
        .expect("failed to spawn kimi-cli-rs binary");

    assert!(
        output.status.success(),
        "expected zero exit in non-interactive default mode"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Non-interactive environment detected"),
        "unexpected stderr: {stderr}"
    );
}
