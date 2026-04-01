use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkillTrust {
    Bundled, // Shipped with the app, highest trust
    #[default]
    Installed, // User-downloaded, restricted
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillActivation {
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub trust: SkillTrust,
    #[serde(default)]
    pub activation: SkillActivation,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub prompt: String, // skill.md content
}
