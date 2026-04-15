use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub mod glob;
pub mod grep;
pub mod read;
pub mod read_media;
pub mod replace;
pub mod write;

const MAX_LINES: usize = 1000;
const MAX_LINE_LENGTH: usize = 2000;
const MAX_BYTES: usize = 100 * 1024;
const MAX_GLOB_MATCHES: usize = 1000;
const MEDIA_SNIFF_BYTES: usize = 512;

/// File operation action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Read,
    Edit,
    EditOutside,
}

/// Tracks a window of recent file operations.
#[derive(Debug, Clone, Default)]
pub struct FileOpsWindow;

/// Result of file type detection.
#[derive(Debug, Clone)]
struct FileType {
    kind: &'static str,
    mime_type: String,
}

/// Expands a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(path))
    } else if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    }
}

/// Truncates a line to the given maximum length.
fn truncate_line(line: &str, max_len: usize) -> String {
    if line.len() <= max_len {
        line.to_string()
    } else {
        let mut end = max_len;
        while !line.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &line[..end])
    }
}

/// Detects the kind of file based on extension and magic bytes.
fn detect_file_type(path: &Path, header: Option<&[u8]>) -> FileType {
    let suffix = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let known_image: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "ico",
        "heic", "heif", "avif", "svgz",
    ];
    let known_video: &[&str] = &[
        "mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "flv", "3gp", "3g2",
    ];
    let known_non_text: &[&str] = &[
        "icns", "psd", "ai", "eps", "pdf", "doc", "docx", "dot", "dotx", "rtf",
        "odt", "xls", "xlsx", "xlsm", "xlt", "xltx", "xltm", "ods", "ppt", "pptx",
        "pptm", "pps", "ppsx", "odp", "pages", "numbers", "key", "zip", "rar",
        "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "lz4", "br", "cab",
        "ar", "deb", "rpm", "mp3", "wav", "flac", "ogg", "oga", "opus", "aac",
        "m4a", "wma", "ttf", "otf", "woff", "woff2", "exe", "dll", "so", "dylib",
        "bin", "apk", "ipa", "jar", "class", "pyc", "pyo", "wasm", "dmg", "iso",
        "img", "sqlite", "sqlite3", "db", "db3",
    ];

    if known_image.contains(&suffix.as_str()) {
        return FileType {
            kind: "image",
            mime_type: format!("image/{}", suffix),
        };
    }
    if known_video.contains(&suffix.as_str()) {
        return FileType {
            kind: "video",
            mime_type: format!("video/{}", suffix),
        };
    }
    if known_non_text.contains(&suffix.as_str()) {
        return FileType {
            kind: "unknown",
            mime_type: String::new(),
        };
    }

    if let Some(hdr) = header {
        if let Some(ftyp) = sniff_ftyp_brand(hdr) {
            let image_brands = ["avif", "avis", "heic", "heif", "heix", "hevc", "mif1", "msf1"];
            let video_brands = [
                "isom", "iso2", "iso5", "mp41", "mp42", "avc1", "mp4v", "m4v", "qt",
                "3gp4", "3gp5", "3gp6", "3gp7", "3g2",
            ];
            if image_brands.contains(&ftyp.as_str()) {
                return FileType {
                    kind: "image",
                    mime_type: format!("image/{}", ftyp),
                };
            }
            if video_brands.contains(&ftyp.as_str()) {
                return FileType {
                    kind: "video",
                    mime_type: format!("video/{}", ftyp),
                };
            }
        }
        if hdr.starts_with(b"\x89PNG\r\n\x1a\n") {
            return FileType {
                kind: "image",
                mime_type: "image/png".into(),
            };
        }
        if hdr.starts_with(b"\xff\xd8\xff") {
            return FileType {
                kind: "image",
                mime_type: "image/jpeg".into(),
            };
        }
        if hdr.starts_with(b"GIF87a") || hdr.starts_with(b"GIF89a") {
            return FileType {
                kind: "image",
                mime_type: "image/gif".into(),
            };
        }
        if hdr.starts_with(b"BM") {
            return FileType {
                kind: "image",
                mime_type: "image/bmp".into(),
            };
        }
        if hdr.starts_with(b"RIFF") && hdr.len() >= 12 {
            let chunk = &hdr[8..12];
            if chunk == b"WEBP" {
                return FileType {
                    kind: "image",
                    mime_type: "image/webp".into(),
                };
            }
            if chunk == b"AVI " {
                return FileType {
                    kind: "video",
                    mime_type: "video/x-msvideo".into(),
                };
            }
        }
        if hdr.starts_with(b"FLV") {
            return FileType {
                kind: "video",
                mime_type: "video/x-flv".into(),
            };
        }
        if hdr.starts_with(b"\x1a\x45\xdf\xa3") {
            let lowered = hdr.to_ascii_lowercase();
            if lowered.windows(4).any(|w| w == b"webm") {
                return FileType {
                    kind: "video",
                    mime_type: "video/webm".into(),
                };
            }
            if lowered.windows(8).any(|w| w == b"matroska") {
                return FileType {
                    kind: "video",
                    mime_type: "video/x-matroska".into(),
                };
            }
        }
        if hdr.contains(&0u8) {
            return FileType {
                kind: "unknown",
                mime_type: String::new(),
            };
        }
    }

    FileType {
        kind: "text",
        mime_type: "text/plain".into(),
    }
}

fn sniff_ftyp_brand(header: &[u8]) -> Option<String> {
    if header.len() < 12 || &header[4..8] != b"ftyp" {
        return None;
    }
    Some(String::from_utf8_lossy(&header[8..12]).to_lowercase())
}

/// Reads a file from disk.
#[derive(Debug, Clone, Default)]
pub struct ReadFile;

#[async_trait]
impl crate::soul::toolset::Tool for ReadFile {
    fn name(&self) -> &str {
        "ReadFile"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to read" },
                "line_offset": { "type": "integer", "default": 1, "description": "Line to start from (1-based). Negative values read from end." },
                "n_lines": { "type": "integer", "default": MAX_LINES, "description": "Maximum lines to read" }
            },
            "required": ["path"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let path = match arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => expand_tilde(p),
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "File path cannot be empty.".into(),
                };
            }
        };

        let line_offset = arguments.get("line_offset").and_then(|v| v.as_i64()).unwrap_or(1);
        let n_lines = arguments
            .get("n_lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(MAX_LINES);
        let n_lines = n_lines.max(1);

        let meta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("`{}` does not exist: {}", path.display(), e),
                };
            }
        };
        if !meta.is_file() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` is not a file.", path.display()),
            };
        }

        let header = match tokio::fs::read(&path).await {
            Ok(data) => data.into_iter().take(MEDIA_SNIFF_BYTES).collect::<Vec<_>>(),
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to read {}: {}", path.display(), e),
                };
            }
        };

        let ft = detect_file_type(&path, Some(&header));
        if ft.kind == "image" || ft.kind == "video" {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!(
                    "`{}` is a {} file. Use ReadMediaFile for media.",
                    path.display(),
                    ft.kind
                ),
            };
        }
        if ft.kind == "unknown" {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!(
                    "`{}` seems not readable. Use shell commands or proper tools.",
                    path.display()
                ),
            };
        }

        if line_offset < 0 {
            return self.read_tail(&path, line_offset, n_lines).await;
        }
        let line_offset = line_offset as usize;
        self.read_forward(&path, line_offset, n_lines).await
    }
}

impl ReadFile {
    async fn read_forward(
        &self,
        path: &Path,
        line_offset: usize,
        n_lines: usize,
    ) -> crate::soul::message::ToolReturnValue {
        let file = match tokio::fs::File::open(path).await {
            Ok(f) => f,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to open {}: {}", path.display(), e),
                };
            }
        };
        let reader = tokio::io::BufReader::new(file);
        let mut lines = tokio::io::AsyncBufReadExt::lines(reader);

        let mut collected: Vec<String> = Vec::new();
        let mut n_bytes: usize = 0;
        let mut truncated_lines: Vec<usize> = Vec::new();
        let mut max_lines_reached = false;
        let mut max_bytes_reached = false;
        let mut current_line_no: usize = 0;
        let mut collecting = true;

        while let Ok(Some(line)) = lines.next_line().await {
            current_line_no += 1;
            if !collecting {
                continue;
            }
            if current_line_no < line_offset {
                continue;
            }
            let truncated = truncate_line(&line, MAX_LINE_LENGTH);
            if truncated != line {
                truncated_lines.push(current_line_no);
            }
            let line_bytes = truncated.len();
            collected.push(truncated);
            n_bytes += line_bytes;
            if collected.len() >= n_lines {
                collecting = false;
            } else if collected.len() >= MAX_LINES {
                max_lines_reached = true;
                collecting = false;
            } else if n_bytes >= MAX_BYTES {
                max_bytes_reached = true;
                collecting = false;
            }
        }

        let total_lines = current_line_no;
        let start_line = line_offset;
        let mut lines_with_no: Vec<String> = Vec::new();
        for (i, line) in collected.iter().enumerate() {
            lines_with_no.push(format!("{:6}\t{}", start_line + i, line));
        }

        let mut message = if collected.is_empty() {
            "No lines read from file.".into()
        } else {
            format!("{} lines read from file starting from line {}.", collected.len(), start_line)
        };
        message += &format!(" Total lines in file: {total_lines}.");
        if max_lines_reached {
            message += &format!(" Max {MAX_LINES} lines reached.");
        } else if max_bytes_reached {
            message += &format!(" Max {MAX_BYTES} bytes reached.");
        } else if collected.len() < n_lines {
            message += " End of file reached.";
        }
        if !truncated_lines.is_empty() {
            message += &format!(" Lines {:?} were truncated.", truncated_lines);
        }

        crate::soul::message::ToolReturnValue::Ok {
            output: lines_with_no.join("\n"),
            message: Some(message),
        }
    }

    async fn read_tail(
        &self,
        path: &Path,
        line_offset: i64,
        n_lines: usize,
    ) -> crate::soul::message::ToolReturnValue {
        let tail_count = line_offset.abs() as usize;
        let file = match tokio::fs::File::open(path).await {
            Ok(f) => f,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to open {}: {}", path.display(), e),
                };
            }
        };
        let reader = tokio::io::BufReader::new(file);
        let mut lines = tokio::io::AsyncBufReadExt::lines(reader);

        let mut tail_buf: Vec<(usize, String, bool)> = Vec::new();
        let mut current_line_no: usize = 0;

        while let Ok(Some(line)) = lines.next_line().await {
            current_line_no += 1;
            let truncated = truncate_line(&line, MAX_LINE_LENGTH);
            let was_truncated = truncated != line;
            tail_buf.push((current_line_no, truncated, was_truncated));
            if tail_buf.len() > tail_count {
                tail_buf.remove(0);
            }
        }

        let total_lines = current_line_no;
        let line_limit = n_lines.min(MAX_LINES);
        let mut candidates = tail_buf;
        let max_lines_reached = candidates.len() > MAX_LINES && line_limit == MAX_LINES;
        if candidates.len() > line_limit {
            candidates = candidates.split_off(candidates.len() - line_limit);
        }

        let mut total_bytes: usize = 0;
        let mut max_bytes_reached = false;
        for entry in &candidates {
            total_bytes += entry.1.len();
        }
        if total_bytes > MAX_BYTES {
            max_bytes_reached = true;
            let mut kept = 0;
            let mut n_bytes = 0;
            for entry in candidates.iter().rev() {
                n_bytes += entry.1.len();
                if n_bytes > MAX_BYTES {
                    break;
                }
                kept += 1;
            }
            let start = candidates.len().saturating_sub(kept);
            candidates = candidates.split_off(start);
        }

        let mut out_lines: Vec<String> = Vec::new();
        let mut truncated_lines: Vec<usize> = Vec::new();
        for (line_no, line, was_truncated) in &candidates {
            if *was_truncated {
                truncated_lines.push(*line_no);
            }
            out_lines.push(format!("{:6}\t{}", line_no, line));
        }

        let start_line = candidates.first().map(|e| e.0).unwrap_or(total_lines + 1);
        let mut message = if out_lines.is_empty() {
            "No lines read from file.".into()
        } else {
            format!("{} lines read from file starting from line {}.", out_lines.len(), start_line)
        };
        message += &format!(" Total lines in file: {total_lines}.");
        if max_lines_reached {
            message += &format!(" Max {MAX_LINES} lines reached.");
        } else if max_bytes_reached {
            message += &format!(" Max {MAX_BYTES} bytes reached.");
        } else if candidates.len() < n_lines {
            message += " End of file reached.";
        }
        if !truncated_lines.is_empty() {
            message += &format!(" Lines {:?} were truncated.", truncated_lines);
        }

        crate::soul::message::ToolReturnValue::Ok {
            output: out_lines.join("\n"),
            message: Some(message),
        }
    }
}

/// Writes a file to disk.
#[derive(Debug, Clone, Default)]
pub struct WriteFile;

#[async_trait]
impl crate::soul::toolset::Tool for WriteFile {
    fn name(&self) -> &str {
        "WriteFile"
    }

    fn description(&self) -> &str {
        "Write content to a file at the given path."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to write" },
                "content": { "type": "string", "description": "Content to write" },
                "mode": { "type": "string", "enum": ["overwrite", "append"], "default": "overwrite", "description": "Write mode" }
            },
            "required": ["path", "content"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let path = match arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => expand_tilde(p),
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "File path cannot be empty.".into(),
                };
            }
        };
        let content = arguments
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mode = arguments
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("overwrite");

        if mode != "overwrite" && mode != "append" {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Invalid write mode: `{}`. Use `overwrite` or `append`.", mode),
            };
        }

        let parent = match path.parent() {
            Some(p) => p,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("`{}` has no parent directory.", path.display()),
                };
            }
        };
        if !parent.exists() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` parent directory does not exist.", path.display()),
            };
        }

        let result = if mode == "overwrite" {
            tokio::fs::write(&path, &content).await
        } else {
            let mut file = match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to open {}: {}", path.display(), e),
                    };
                }
            };
            use tokio::io::AsyncWriteExt;
            file.write_all(content.as_bytes()).await
        };

        match result {
            Ok(_) => {
                let size = match tokio::fs::metadata(&path).await {
                    Ok(m) => m.len(),
                    Err(_) => content.len() as u64,
                };
                let action = if mode == "overwrite" {
                    "overwritten"
                } else {
                    "appended to"
                };
                crate::soul::message::ToolReturnValue::Ok {
                    output: String::new(),
                    message: Some(format!(
                        "File successfully {action}. Current size: {size} bytes."
                    )),
                }
            }
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to write {}: {}", path.display(), e),
            },
        }
    }
}

/// Single string replacement edit.
#[derive(Debug, Clone, serde::Deserialize)]
struct Edit {
    old: String,
    new: String,
    #[serde(default)]
    replace_all: bool,
}

/// Replaces content in a file.
#[derive(Debug, Clone, Default)]
pub struct StrReplaceFile;

#[async_trait]
impl crate::soul::toolset::Tool for StrReplaceFile {
    fn name(&self) -> &str {
        "StrReplaceFile"
    }

    fn description(&self) -> &str {
        "Replace a string in a file with another string."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "edit": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "old": { "type": "string" },
                                "new": { "type": "string" },
                                "replace_all": { "type": "boolean", "default": false }
                            },
                            "required": ["old", "new"]
                        },
                        {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old": { "type": "string" },
                                    "new": { "type": "string" },
                                    "replace_all": { "type": "boolean", "default": false }
                                },
                                "required": ["old", "new"]
                            }
                        }
                    ]
                }
            },
            "required": ["path", "edit"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let path = match arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => expand_tilde(p),
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "File path cannot be empty.".into(),
                };
            }
        };

        let edits: Vec<Edit> = match arguments.get("edit") {
            Some(Value::Array(arr)) => {
                let mut out = Vec::new();
                for v in arr {
                    match serde_json::from_value::<Edit>(v.clone()) {
                        Ok(e) => out.push(e),
                        Err(e) => {
                            return crate::soul::message::ToolReturnValue::Error {
                                error: format!("Invalid edit object: {}", e),
                            };
                        }
                    }
                }
                out
            }
            Some(v) => match serde_json::from_value::<Edit>(v.clone()) {
                Ok(e) => vec![e],
                Err(e) => {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Invalid edit object: {}", e),
                    };
                }
            },
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Missing edit parameter.".into(),
                };
            }
        };

        if !path.exists() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` does not exist.", path.display()),
            };
        }
        if !path.is_file() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` is not a file.", path.display()),
            };
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to read {}: {}", path.display(), e),
                };
            }
        };

        let original = content.clone();
        let mut current = content;
        for edit in &edits {
            if edit.replace_all {
                current = current.replace(&edit.old, &edit.new);
            } else {
                current = current.replacen(&edit.old, &edit.new, 1);
            }
        }

        if current == original {
            return crate::soul::message::ToolReturnValue::Error {
                error: "No replacements were made. The old string was not found in the file.".into(),
            };
        }

        if let Err(e) = tokio::fs::write(&path, current).await {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to write {}: {}", path.display(), e),
            };
        }

        let total_replacements: usize = edits
            .iter()
            .map(|e| {
                if e.replace_all {
                    original.matches(&e.old).count()
                } else {
                    if original.contains(&e.old) { 1 } else { 0 }
                }
            })
            .sum();

        crate::soul::message::ToolReturnValue::Ok {
            output: String::new(),
            message: Some(format!(
                "File successfully edited. Applied {} edit(s) with {} total replacement(s).",
                edits.len(),
                total_replacements
            )),
        }
    }
}

/// Glob file search.
#[derive(Debug, Clone, Default)]
pub struct Glob;

#[async_trait]
impl crate::soul::toolset::Tool for Glob {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern to match" },
                "directory": { "type": "string", "description": "Directory to search in (defaults to current working directory)", "default": "." },
                "include_dirs": { "type": "boolean", "default": true, "description": "Include directories in results" }
            },
            "required": ["pattern"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let pattern = match arguments.get("pattern").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p,
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Pattern cannot be empty.".into(),
                };
            }
        };

        if pattern.starts_with("**") {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Patterns starting with '**' are not allowed. Use a more specific pattern.".into(),
            };
        }

        let directory = arguments
            .get("directory")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let include_dirs = arguments
            .get("include_dirs")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let base = if directory == "." {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            expand_tilde(directory)
        };

        if !base.exists() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` does not exist.", base.display()),
            };
        }
        if !base.is_dir() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` is not a directory.", base.display()),
            };
        }

        let full_pattern = base.join(pattern);
        let pattern_str = match full_pattern.to_str() {
            Some(s) => s,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Invalid glob pattern encoding.".into(),
                };
            }
        };

        let mut matches: Vec<PathBuf> = Vec::new();
        match ::glob::glob(pattern_str) {
            Ok(paths) => {
                for entry in paths {
                    match entry {
                        Ok(path) => {
                            let include = if include_dirs {
                                true
                            } else {
                                path.is_file()
                            };
                            if include {
                                matches.push(path);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Invalid glob pattern: {}", e),
                };
            }
        };

        matches.sort();
        let truncated = matches.len() > MAX_GLOB_MATCHES;
        if truncated {
            matches.truncate(MAX_GLOB_MATCHES);
        }

        let output = matches
            .iter()
            .filter_map(|p| p.strip_prefix(&base).ok().or(Some(p.as_path())))
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        let mut message = if matches.is_empty() {
            format!("No matches found for pattern `{pattern}`.")
        } else {
            format!("Found {} matches for pattern `{pattern}`.", matches.len())
        };
        if truncated {
            message += &format!(
                " Only the first {MAX_GLOB_MATCHES} matches are returned. Use a more specific pattern."
            );
        }

        crate::soul::message::ToolReturnValue::Ok {
            output,
            message: Some(message),
        }
    }
}

/// Grep file search.
#[derive(Debug, Clone, Default)]
pub struct Grep;

#[async_trait]
impl crate::soul::toolset::Tool for Grep {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents for a regex pattern."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "Directory or file to search in", "default": "." },
                "output_mode": { "type": "string", "enum": ["content", "files_with_matches", "count_matches"], "default": "files_with_matches" },
                "ignore_case": { "type": "boolean", "default": false },
                "multiline": { "type": "boolean", "default": false },
                "glob": { "type": "string", "description": "Glob filter for files" },
                "head_limit": { "type": "integer", "default": 250 },
                "offset": { "type": "integer", "default": 0 }
            },
            "required": ["pattern"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let pattern_str = match arguments.get("pattern").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p,
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Pattern cannot be empty.".into(),
                };
            }
        };

        let path_arg = arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let output_mode = arguments
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("files_with_matches");
        let ignore_case = arguments
            .get("ignore_case")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let multiline = arguments
            .get("multiline")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let glob_filter = arguments.get("glob").and_then(|v| v.as_str());
        let head_limit = arguments
            .get("head_limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(250);
        let offset = arguments
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(0);

        let search_path = if path_arg == "." {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            expand_tilde(path_arg)
        };

        let mut regex_builder = regex::RegexBuilder::new(pattern_str);
        if ignore_case {
            regex_builder.case_insensitive(true);
        }
        if multiline {
            regex_builder.multi_line(true).dot_matches_new_line(true);
        }
        let regex = match regex_builder.build() {
            Ok(r) => r,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Invalid regex pattern: {}", e),
                };
            }
        };

        let files = if search_path.is_file() {
            vec![search_path.clone()]
        } else if search_path.is_dir() {
            match collect_files(&search_path, glob_filter).await {
                Ok(f) => f,
                Err(e) => {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to list files: {}", e),
                    };
                }
            }
        } else {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` does not exist.", search_path.display()),
            };
        };

        let mut results: Vec<String> = Vec::new();
        let mut match_count: usize = 0;
        let mut matched_files: HashSet<String> = HashSet::new();

        for file in files {
            let text = match tokio::fs::read_to_string(&file).await {
                Ok(t) => t,
                Err(_) => continue,
            };

            if output_mode == "files_with_matches" {
                if regex.is_match(&text) {
                    matched_files.insert(file.display().to_string());
                }
            } else if output_mode == "count_matches" {
                let count = regex.find_iter(&text).count();
                if count > 0 {
                    match_count += count;
                    matched_files.insert(format!("{}:{}", file.display(), count));
                }
            } else {
                // content mode: show matching lines with line numbers
                for (i, line) in text.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(format!("{}:{}:{}", file.display(), i + 1, line));
                    }
                }
            }
        }

        let mut output_lines: Vec<String> = if output_mode == "files_with_matches" {
            matched_files.into_iter().collect()
        } else if output_mode == "count_matches" {
            matched_files.into_iter().collect()
        } else {
            results
        };
        output_lines.sort();

        if offset > 0 && offset < output_lines.len() {
            output_lines = output_lines.split_off(offset);
        } else if offset >= output_lines.len() {
            output_lines.clear();
        }

        let total_before_limit = output_lines.len();
        if head_limit > 0 && output_lines.len() > head_limit {
            output_lines.truncate(head_limit);
        }

        let output = output_lines.join("\n");
        let mut message = String::new();
        if output_mode == "count_matches" {
            message = format!("Found {match_count} total occurrences.");
        } else if output_lines.is_empty() {
            message = "No matches found.".into();
        }
        if head_limit > 0 && total_before_limit > head_limit {
            message += &format!(
                " Results truncated to {head_limit} lines. Use offset={} to see more.",
                offset + head_limit
            );
        }

        crate::soul::message::ToolReturnValue::Ok {
            output,
            message: if message.is_empty() { None } else { Some(message) },
        }
    }
}

async fn collect_files(dir: &Path, glob_filter: Option<&str>) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(current).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name == ".git" || name == "target" || name == "node_modules" {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() {
                if let Some(filter) = glob_filter {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !::glob::Pattern::new(filter)
                            .map(|p| p.matches(name))
                            .unwrap_or(false)
                        {
                            continue;
                        }
                    }
                }
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// Reads media files (images, audio, video).
#[derive(Debug, Clone, Default)]
pub struct ReadMediaFile;

#[async_trait]
impl crate::soul::toolset::Tool for ReadMediaFile {
    fn name(&self) -> &str {
        "ReadMediaFile"
    }

    fn description(&self) -> &str {
        "Read a media file and return its base64-encoded contents."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the media file" }
            },
            "required": ["path"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let path = match arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => expand_tilde(p),
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "File path cannot be empty.".into(),
                };
            }
        };

        if !path.exists() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` does not exist.", path.display()),
            };
        }
        if !path.is_file() {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("`{}` is not a file.", path.display()),
            };
        }

        match tokio::fs::read(&path).await {
            Ok(data) => {
                let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);
                let len = encoded.len();
                crate::soul::message::ToolReturnValue::Ok {
                    output: encoded,
                    message: Some(format!("Encoded {} ({} bytes)", path.display(), len)),
                }
            }
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to read {}: {}", path.display(), e),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_home() {
        let expanded = expand_tilde("~/test");
        assert!(!expanded.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn detect_file_type_text() {
        let ft = detect_file_type(Path::new("foo.rs"), None);
        assert_eq!(ft.kind, "text");
    }

    #[test]
    fn detect_file_type_image_by_extension() {
        let ft = detect_file_type(Path::new("foo.png"), None);
        assert_eq!(ft.kind, "image");
    }

    #[test]
    fn detect_file_type_binary_by_nul() {
        let ft = detect_file_type(Path::new("foo.bin"), Some(b"hello\x00world"));
        assert_eq!(ft.kind, "unknown");
    }

    #[test]
    fn truncate_line_short() {
        assert_eq!(truncate_line("hi", 10), "hi");
    }

    #[test]
    fn truncate_line_long() {
        let s = "a".repeat(3000);
        let out = truncate_line(&s, MAX_LINE_LENGTH);
        assert!(out.ends_with("..."));
        assert!(out.len() <= MAX_LINE_LENGTH + 3);
    }
}
