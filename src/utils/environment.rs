use std::path::PathBuf;

/// Detected system environment.
#[derive(Debug, Clone)]
pub struct Environment {
    pub os_kind: String,
    pub os_arch: String,
    pub os_version: String,
    pub shell_name: String,
    pub shell_path: PathBuf,
}

impl Environment {
    /// Detects the current environment.
    #[tracing::instrument(level = "debug")]
    pub async fn detect() -> Self {
        let os_kind = match std::env::consts::OS {
            "macos" => "macOS",
            "windows" => "Windows",
            "linux" => "Linux",
            other => other,
        }
        .to_string();

        let os_arch = std::env::consts::ARCH.to_string();
        let os_version = sysinfo::System::os_version().unwrap_or_default();

        let (shell_name, shell_path) = if os_kind == "Windows" {
            let system_root = std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".into());
            let possible = PathBuf::from(&system_root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe");
            if possible.is_file() {
                ("Windows PowerShell".into(), possible)
            } else {
                ("Windows PowerShell".into(), PathBuf::from("powershell.exe"))
            }
        } else {
            let possible = [
                PathBuf::from("/bin/bash"),
                PathBuf::from("/usr/bin/bash"),
                PathBuf::from("/usr/local/bin/bash"),
            ];
            let mut found = None;
            for path in &possible {
                if path.is_file() {
                    found = Some(path.clone());
                    break;
                }
            }
            if let Some(path) = found {
                ("bash".into(), path)
            } else {
                ("sh".into(), PathBuf::from("/bin/sh"))
            }
        };

        Self {
            os_kind,
            os_arch,
            os_version,
            shell_name,
            shell_path,
        }
    }
}
