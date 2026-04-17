/// Generates a human-readable summary of a background task.
pub fn summarize(task: &crate::background::manager::BackgroundTask) -> String {
    let status = if task.is_running_blocking() {
        "running"
    } else if task.exit_code_blocking() == Some(0) {
        "completed"
    } else {
        "failed"
    };
    format!("{} [{}] {}", task.id, status, task.command)
}
