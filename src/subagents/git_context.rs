use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_DIRTY_FILES: usize = 20;
const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "gitlab.com",
    "gitee.com",
    "bitbucket.org",
    "codeberg.org",
    "sr.ht",
];

/// Collect git repository context for the explore agent.
pub async fn collect_git_context(work_dir: &Path) -> String {
    let cwd = work_dir.to_string_lossy();

    // Quick check: is this a git repo?
    if run_git(&["rev-parse", "--is-inside-work-tree"], &cwd).await.is_none() {
        return String::new();
    }

    let (remote_url, branch, dirty_raw, log_raw) = tokio::join!(
        run_git(&["remote", "get-url", "origin"], &cwd),
        run_git(&["branch", "--show-current"], &cwd),
        run_git(&["status", "--porcelain"], &cwd),
        run_git(&["log", "-3", "--format=%h %s"], &cwd),
    );

    let mut sections = vec![format!("Working directory: {cwd}")];

    if let Some(url) = remote_url {
        if let Some(safe) = sanitize_remote_url(&url) {
            sections.push(format!("Remote: {safe}"));
        }
        if let Some(project) = parse_project_name(&url) {
            sections.push(format!("Project: {project}"));
        }
    }

    if let Some(b) = branch {
        sections.push(format!("Branch: {b}"));
    }

    if let Some(dirty) = dirty_raw {
        let lines: Vec<_> = dirty.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            let total = lines.len();
            let shown = &lines[..total.min(MAX_DIRTY_FILES)];
            let mut body = shown.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n");
            if total > MAX_DIRTY_FILES {
                body.push_str(&format!("\n  ... and {} more", total - MAX_DIRTY_FILES));
            }
            sections.push(format!("Dirty files ({total}):\n{body}"));
        }
    }

    if let Some(log) = log_raw {
        let lines: Vec<_> = log.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            let body = lines.iter().map(|l| format!("  {}", &l[..l.len().min(200)])).collect::<Vec<_>>().join("\n");
            sections.push(format!("Recent commits:\n{body}"));
        }
    }

    if sections.len() <= 1 {
        return String::new();
    }

    format!("<git-context>\n{}\n</git-context>", sections.join("\n"))
}

async fn run_git(args: &[&str], cwd: &str) -> Option<String> {
    let mut cmd = crate::utils::subprocess_env::git_tokio_command();
    cmd.arg("-C").arg(cwd).args(args);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());

    match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(Ok(_)) => None,
        Ok(Err(e)) => {
            tracing::debug!("git command failed: {}", e);
            None
        }
        Err(_) => {
            tracing::debug!("git command timed out");
            None
        }
    }
}

fn sanitize_remote_url(remote_url: &str) -> Option<String> {
    // SSH format: git@host:owner/repo.git
    for host in ALLOWED_HOSTS {
        let prefix = format!("git@{host}:");
        if remote_url.starts_with(&prefix) {
            return Some(remote_url.to_string());
        }
    }

    // HTTPS format
    if let Ok(url) = url::Url::parse(remote_url) {
        if let Some(hostname) = url.host_str() {
            if ALLOWED_HOSTS.contains(&hostname) {
                let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
                return Some(format!("https://{}{}{}", hostname, port, url.path()));
            }
        }
    }

    None
}

fn parse_project_name(remote_url: &str) -> Option<String> {
    // SSH format: git@host:owner/repo.git
    if let Some(idx) = remote_url.rfind(':') {
        let after = &remote_url[idx + 1..];
        if let Some(slash) = after.find('/') {
            let rest = &after[slash + 1..];
            let end = rest.find(".git").unwrap_or(rest.len());
            return Some(format!("{}/{}", &after[..slash], &rest[..end]));
        }
    }
    // HTTPS format
    let parts: Vec<_> = remote_url.split('/').collect();
    if parts.len() >= 2 {
        let owner = parts[parts.len() - 2];
        let repo = parts[parts.len() - 1];
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        return Some(format!("{owner}/{repo}"));
    }
    None
}
