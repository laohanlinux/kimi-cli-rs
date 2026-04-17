pub mod flow;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Skill type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillType {
    #[default]
    Standard,
    Flow,
}

impl std::str::FromStr for SkillType {
    type Err = crate::error::KimiCliError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "standard" => Ok(SkillType::Standard),
            "flow" => Ok(SkillType::Flow),
            _ => Err(crate::error::KimiCliError::Config(
                format!("Invalid skill type: {s}").into(),
            )),
        }
    }
}

/// Information about a single skill.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub r#type: SkillType,
    pub dir: PathBuf,
    pub flow: Option<crate::skill::flow::Flow>,
}

impl Default for Skill {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: "No description provided.".into(),
            r#type: SkillType::Standard,
            dir: PathBuf::new(),
            flow: None,
        }
    }
}

impl Skill {
    /// Path to the SKILL.md file.
    pub fn skill_md_file(&self) -> PathBuf {
        self.dir.join("SKILL.md")
    }
}

/// Returns the built-in skills directory path.
pub fn get_builtin_skills_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills")
}

/// Normalizes a skill name for lookup.
pub fn normalize_skill_name(name: &str) -> String {
    name.to_lowercase()
}

/// Builds a lookup table for skills by normalized name.
pub fn index_skills(skills: &[Skill]) -> HashMap<String, &Skill> {
    skills
        .iter()
        .map(|skill| (normalize_skill_name(&skill.name), skill))
        .collect()
}

/// Discovers skills from the given directory.
#[tracing::instrument(level = "debug")]
pub async fn discover_skills(skills_dir: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut entries = match tokio::fs::read_dir(skills_dir).await {
        Ok(e) => e,
        Err(_) => return skills,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        match tokio::fs::read_to_string(&skill_md).await {
            Ok(content) => match parse_skill_text(&content, &path) {
                Ok(skill) => skills.push(skill),
                Err(e) => {
                    tracing::warn!("Skipping invalid skill at {}: {}", skill_md.display(), e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read skill file {}: {}", skill_md.display(), e);
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Discovers skills from multiple directory roots.
#[tracing::instrument(level = "debug")]
pub async fn discover_skills_from_roots(skills_dirs: &[PathBuf]) -> Vec<Skill> {
    let mut skills_by_name: HashMap<String, Skill> = HashMap::new();
    for dir in skills_dirs {
        for skill in discover_skills(dir).await {
            let key = normalize_skill_name(&skill.name);
            skills_by_name.entry(key).or_insert(skill);
        }
    }
    let mut skills: Vec<_> = skills_by_name.into_values().collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Reads the SKILL.md contents for a skill.
#[tracing::instrument(level = "debug", skip(skill))]
pub async fn read_skill_text(skill: &Skill) -> Option<String> {
    match tokio::fs::read_to_string(skill.skill_md_file()).await {
        Ok(text) => Some(text.trim().to_string()),
        Err(e) => {
            tracing::warn!(
                "Failed to read skill file {}: {}",
                skill.skill_md_file().display(),
                e
            );
            None
        }
    }
}

/// Parses SKILL.md contents to extract name, description, and type from YAML frontmatter.
/// For flow skills, also parses the first mermaid or d2 fenced code block into a Flow.
#[tracing::instrument(level = "debug")]
pub fn parse_skill_text(content: &str, dir_path: &Path) -> crate::error::Result<Skill> {
    let mut name = dir_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut description = "No description provided.".to_string();
    let mut skill_type = SkillType::Standard;

    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        if let Some(end) = trimmed[3..].find("\n---") {
            let frontmatter = &trimmed[3..3 + end];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    match key {
                        "name" => name = value.to_string(),
                        "description" => description = value.to_string(),
                        "type" => {
                            skill_type = value.parse().unwrap_or(SkillType::Standard);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let mut flow = None;
    if skill_type == SkillType::Flow {
        match parse_flow_from_skill(content) {
            Ok(f) => flow = Some(f),
            Err(e) => {
                tracing::error!(
                    "Failed to parse flow skill {}: {}. Falling back to standard.",
                    name,
                    e
                );
                skill_type = SkillType::Standard;
            }
        }
    }

    Ok(Skill {
        name,
        description,
        r#type: skill_type,
        dir: dir_path.to_path_buf(),
        flow,
    })
}

fn parse_flow_from_skill(content: &str) -> crate::error::Result<crate::skill::flow::Flow> {
    for (lang, code) in iter_fenced_codeblocks(content) {
        if lang == "mermaid" {
            return crate::skill::flow::mermaid::parse_mermaid_flowchart(&code).map_err(|e| {
                crate::error::KimiCliError::Generic(
                    format!("Invalid mermaid flow diagram: {e}").into(),
                )
            });
        }
        if lang == "d2" {
            return crate::skill::flow::d2::parse_d2_flowchart(&code).map_err(|e| {
                crate::error::KimiCliError::Generic(format!("Invalid d2 flow diagram: {e}").into())
            });
        }
    }
    Err(crate::error::KimiCliError::Generic(
        "Flow skills require a mermaid or d2 code block in SKILL.md.".into(),
    ))
}

fn iter_fenced_codeblocks(content: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let mut fence = String::new();
    let mut fence_char = '\0';
    let mut lang = String::new();
    let mut buf: Vec<&str> = Vec::new();
    let mut in_block = false;

    for line in content.lines() {
        let stripped = line.trim_start();
        if !in_block {
            if let Some((f, fc, info)) = parse_fence_open(stripped) {
                fence = f;
                fence_char = fc;
                lang = normalize_code_lang(&info);
                in_block = true;
                buf.clear();
            }
            continue;
        }

        if is_fence_close(stripped, fence_char, fence.len()) {
            result.push((lang.clone(), buf.join("\n").trim_matches('\n').to_string()));
            in_block = false;
            fence.clear();
            fence_char = '\0';
            lang.clear();
            buf.clear();
            continue;
        }

        buf.push(line);
    }

    result
}

fn parse_fence_open(line: &str) -> Option<(String, char, String)> {
    if line.is_empty() {
        return None;
    }
    let first = line.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let mut count = 0;
    for ch in line.chars() {
        if ch == first {
            count += 1;
        } else {
            break;
        }
    }
    if count < 3 {
        return None;
    }
    let fence = first.to_string().repeat(count);
    let info = line[count..].trim().to_string();
    Some((fence, first, info))
}

fn normalize_code_lang(info: &str) -> String {
    if info.is_empty() {
        return String::new();
    }
    let lang = info
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if lang.starts_with('{') && lang.ends_with('}') {
        lang[1..lang.len() - 1].trim().to_string()
    } else {
        lang
    }
}

fn is_fence_close(line: &str, fence_char: char, fence_len: usize) -> bool {
    if fence_char == '\0' || line.is_empty() || line.chars().next() != Some(fence_char) {
        return false;
    }
    let mut count = 0;
    for ch in line.chars() {
        if ch == fence_char {
            count += 1;
        } else {
            break;
        }
    }
    if count < fence_len {
        return false;
    }
    line[count..].trim().is_empty()
}

/// Resolves layered skill roots in priority order.
#[tracing::instrument(level = "debug")]
pub async fn resolve_skills_roots(
    work_dir: &Path,
    skills_dirs: Option<&[PathBuf]>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(get_builtin_skills_dir());

    // User-level skills directory
    if let Some(config_dir) = dirs::config_dir() {
        let user_skills = config_dir.join("kimi").join("skills");
        if user_skills.is_dir() {
            roots.push(user_skills);
        }
    }

    // Project-level skills directory
    let project_skills = work_dir.join(".kimi").join("skills");
    if project_skills.is_dir() {
        roots.push(project_skills);
    }

    if let Some(dirs) = skills_dirs {
        roots.extend(dirs.iter().cloned());
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_skill_text_uses_defaults_without_frontmatter() {
        let dir = PathBuf::from("/tmp/skills/my-skill");
        let skill = parse_skill_text("Some content", &dir).unwrap();
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "No description provided.");
        assert_eq!(skill.r#type, SkillType::Standard);
    }

    #[test]
    fn parse_skill_text_extracts_frontmatter() {
        let dir = PathBuf::from("/tmp/skills/ignored-name");
        let content = r#"---
name: Test Skill
description: A test description
type: flow
---

# Content

```mermaid
flowchart TD
    A[begin] --> B[end]
```
"#;
        let skill = parse_skill_text(content, &dir).unwrap();
        assert_eq!(skill.name, "Test Skill");
        assert_eq!(skill.description, "A test description");
        assert_eq!(skill.r#type, SkillType::Flow);
        assert!(skill.flow.is_some());
    }

    #[test]
    fn parse_skill_text_partial_frontmatter() {
        let dir = PathBuf::from("/tmp/skills/my-skill");
        let content = r#"---
name: Partial Skill
---
Content here."#;
        let skill = parse_skill_text(content, &dir).unwrap();
        assert_eq!(skill.name, "Partial Skill");
        assert_eq!(skill.description, "No description provided.");
        assert_eq!(skill.r#type, SkillType::Standard);
    }

    #[test]
    fn parse_skill_text_invalid_type_fallback() {
        let dir = PathBuf::from("/tmp/skills/my-skill");
        let content = r#"---
type: unknown_type
---
"#;
        let skill = parse_skill_text(content, &dir).unwrap();
        assert_eq!(skill.r#type, SkillType::Standard);
    }
}
