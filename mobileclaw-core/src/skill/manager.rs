use super::types::Skill;

pub struct SkillManager {
    skills: Vec<Skill>,
}

impl SkillManager {
    pub fn new(skills: Vec<Skill>) -> Self {
        Self { skills }
    }

    pub fn match_skills(&self, input: &str) -> Vec<&Skill> {
        let input_lower = input.to_lowercase();
        self.skills
            .iter()
            .filter(|s| {
                s.manifest
                    .activation
                    .keywords
                    .iter()
                    .any(|kw| input_lower.contains(&kw.to_lowercase()))
            })
            .collect()
    }

    pub fn build_system_prompt(&self, base_system: &str, matched: &[&Skill]) -> String {
        if matched.is_empty() {
            return base_system.to_string();
        }
        let skill_prompts: String = matched
            .iter()
            .map(|s| format!("\n\n---\n## Skill: {}\n\n{}", s.manifest.name, s.prompt))
            .collect();
        format!("{}{}", base_system, skill_prompts)
    }

    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::types::{SkillActivation, SkillManifest, SkillTrust};

    fn make_skill(name: &str, keywords: Vec<&str>) -> Skill {
        Skill {
            manifest: SkillManifest {
                name: name.into(),
                description: "test".into(),
                trust: SkillTrust::Bundled,
                activation: SkillActivation {
                    keywords: keywords.into_iter().map(String::from).collect(),
                },
                allowed_tools: None,
            },
            prompt: format!("You are the {} skill.", name),
        }
    }

    #[test]
    fn keyword_match_is_case_insensitive() {
        let mgr = SkillManager::new(vec![make_skill("review", vec!["review", "代码审查"])]);
        assert_eq!(mgr.match_skills("Please REVIEW my code").len(), 1);
        assert_eq!(mgr.match_skills("请帮我代码审查").len(), 1);
        assert_eq!(mgr.match_skills("hello world").len(), 0);
    }

    #[test]
    fn build_system_prompt_appends_skill_prompts() {
        let mgr = SkillManager::new(vec![make_skill("review", vec!["review"])]);
        let matched = mgr.match_skills("review code");
        let prompt = mgr.build_system_prompt("Base system.", &matched);
        assert!(prompt.starts_with("Base system."));
        assert!(prompt.contains("review skill"));
    }
}
