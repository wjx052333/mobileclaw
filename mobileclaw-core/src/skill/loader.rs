use std::path::Path;
use tracing::warn;
use crate::ClawResult;
use super::types::{Skill, SkillManifest};

pub async fn load_skills_from_dir(dir: &Path) -> ClawResult<Vec<Skill>> {
    let mut skills = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let yaml_path = path.join("skill.yaml");
        let md_path = path.join("skill.md");
        if !yaml_path.exists() || !md_path.exists() {
            continue;
        }

        let yaml_str = match tokio::fs::read_to_string(&yaml_path).await {
            Ok(s) => s,
            Err(e) => {
                warn!("skipping skill {:?}: read error {}", path, e);
                continue;
            }
        };
        let manifest: SkillManifest = match serde_yaml::from_str(&yaml_str) {
            Ok(m) => m,
            Err(e) => {
                warn!("skipping skill {:?}: YAML parse error {}", path, e);
                continue;
            }
        };
        let prompt = match tokio::fs::read_to_string(&md_path).await {
            Ok(s) => s,
            Err(e) => {
                warn!("skipping skill {:?}: read prompt error {}", path, e);
                continue;
            }
        };
        skills.push(Skill { manifest, prompt });
    }
    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(dir: &TempDir, subdir: &str, yaml: &str, md: &str) {
        let skill_dir = dir.path().join(subdir);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("skill.yaml"), yaml).unwrap();
        std::fs::write(skill_dir.join("skill.md"), md).unwrap();
    }

    #[tokio::test]
    async fn load_valid_skill() {
        let dir = TempDir::new().unwrap();
        write_skill(
            &dir,
            "code-review",
            r#"
name: code-review
description: 代码审查助手
trust: installed
activation:
  keywords: ["review", "代码审查"]
"#,
            "# Code Review\n你是代码审查专家。",
        );
        let skills = load_skills_from_dir(dir.path()).await.unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].manifest.name, "code-review");
        assert!(skills[0].prompt.contains("代码审查专家"));
    }

    #[tokio::test]
    async fn skip_invalid_skill_yaml() {
        let dir = TempDir::new().unwrap();
        write_skill(&dir, "bad-skill", "not: valid: yaml: {{{{", "# bad");
        let skills = load_skills_from_dir(dir.path()).await.unwrap();
        assert_eq!(skills.len(), 0);
    }

    #[tokio::test]
    async fn empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let skills = load_skills_from_dir(dir.path()).await.unwrap();
        assert!(skills.is_empty());
    }
}
