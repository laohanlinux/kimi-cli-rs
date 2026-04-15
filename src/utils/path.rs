use std::path::{Path, PathBuf};

const LIST_DIR_ROOT_WIDTH: usize = 30;
const LIST_DIR_CHILD_WIDTH: usize = 10;

#[derive(Debug)]
struct DirEntry {
    name: String,
    is_dir: bool,
}

async fn collect_entries(dir_path: &Path, max_width: usize) -> std::io::Result<(Vec<DirEntry>, usize)> {
    let mut entries = Vec::new();
    let mut total = 0usize;
    let mut read_dir = tokio::fs::read_dir(dir_path).await?;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        total += 1;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false);
        entries.push(DirEntry { name, is_dir });
    }
    entries.sort_by(|a, b| {
        let a_dir = if a.is_dir { 0 } else { 1 };
        let b_dir = if b.is_dir { 0 } else { 1 };
        (a_dir, &a.name).cmp(&(b_dir, &b.name))
    });
    let truncated = entries.into_iter().take(max_width).collect();
    Ok((truncated, total))
}

/// Returns a compact tree listing of *work_dir* (up to 2 levels).
pub async fn list_directory(work_dir: &Path) -> String {
    let Ok((entries, total)) = collect_entries(work_dir, LIST_DIR_ROOT_WIDTH).await else {
        return "(directory not readable)".into();
    };
    let remaining = total.saturating_sub(entries.len());
    let mut lines: Vec<String> = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let is_last = (i == entries.len() - 1) && remaining == 0;
        let connector = if is_last { "└── " } else { "├── " };
        if entry.is_dir {
            lines.push(format!("{connector}{}/", entry.name));
            let child_prefix = if is_last { "    " } else { "│   " };
            let child_path = work_dir.join(&entry.name);
            match collect_entries(&child_path, LIST_DIR_CHILD_WIDTH).await {
                Ok((child_entries, child_total)) => {
                    let child_remaining = child_total.saturating_sub(child_entries.len());
                    for (j, child) in child_entries.iter().enumerate() {
                        let child_is_last = (j == child_entries.len() - 1) && child_remaining == 0;
                        let child_connector = if child_is_last { "└── " } else { "├── " };
                        let suffix = if child.is_dir { "/" } else { "" };
                        lines.push(format!("{child_prefix}{child_connector}{}{suffix}", child.name));
                    }
                    if child_remaining > 0 {
                        lines.push(format!("{child_prefix}└── ... and {child_remaining} more"));
                    }
                }
                Err(_) => {
                    lines.push(format!("{child_prefix}└── [not readable]"));
                }
            }
        } else {
            lines.push(format!("{connector}{}", entry.name));
        }
    }

    if remaining > 0 {
        lines.push(format!("└── ... and {remaining} more entries"));
    }

    if lines.is_empty() {
        "(empty directory)".into()
    } else {
        lines.join("\n")
    }
}

/// Checks whether *path* is contained within *directory* using pure path semantics.
pub fn is_within_directory(path: &Path, directory: &Path) -> bool {
    path.strip_prefix(directory).is_ok()
}
