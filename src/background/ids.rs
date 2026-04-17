/// Generates a background task ID.
pub fn generate_task_id(prefix: &str) -> String {
    format!("{}-{}", prefix, uuid::Uuid::new_v4())
}
