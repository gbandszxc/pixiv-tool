//! 内置资源：编译期嵌入、按 ID 加载、内容版本哈希。
//!
//! 运行时不读取源码树或用户目录；资源随二进制发布。资源 ID 是唯一入口，
//! 调用方不能传任意路径。`prompt:system` 只供宿主装配系统提示词，**不**出现在
//! `load_resource` 的允许集合里，避免模型工具自行改写系统提示。

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::Role;

// 编译期嵌入：路径相对本文件（src-tauri/src/agent/resources.rs）。
const SYSTEM_PROMPT: &str = include_str!("../../resources/agent/prompts/system.md");
const AGENT_COORDINATOR: &str = include_str!("../../resources/agent/agents/coordinator.md");
const AGENT_TRANSLATOR: &str = include_str!("../../resources/agent/agents/translator.md");
const AGENT_REVIEWER: &str = include_str!("../../resources/agent/agents/reviewer.md");
const SKILL: &str = include_str!("../../resources/agent/skills/smart-translator/SKILL.md");
const REF_LITERATURE: &str =
    include_str!("../../resources/agent/skills/smart-translator/references/domain-literature.md");
const REF_ACG: &str =
    include_str!("../../resources/agent/skills/smart-translator/references/domain-acg.md");
const REF_ACADEMIC: &str =
    include_str!("../../resources/agent/skills/smart-translator/references/domain-academic.md");
const REF_NEWS: &str =
    include_str!("../../resources/agent/skills/smart-translator/references/domain-news.md");
const REF_TECH: &str =
    include_str!("../../resources/agent/skills/smart-translator/references/domain-tech.md");
const REF_LEGAL: &str =
    include_str!("../../resources/agent/skills/smart-translator/references/domain-legal.md");

/// 可加载的领域参考资料。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Domain {
    Literature,
    Acg,
    Academic,
    News,
    Tech,
    Legal,
}

impl Domain {
    pub const ALL: [Domain; 6] = [
        Domain::Literature,
        Domain::Acg,
        Domain::Academic,
        Domain::News,
        Domain::Tech,
        Domain::Legal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Literature => "literature",
            Self::Acg => "acg",
            Self::Academic => "academic",
            Self::News => "news",
            Self::Tech => "tech",
            Self::Legal => "legal",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        // 资源 ID 使用 `reference:domain-<name>`；同时容忍不带前缀的短名。
        let name = value.strip_prefix("domain-").unwrap_or(value);
        Self::ALL.into_iter().find(|domain| domain.as_str() == name)
    }
}

/// 模型可请求的资源 ID。
///
/// 解析只接受固定集合；`prompt:system` 与任何路径形态（`/`、`\`、`..`）都拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ResourceId {
    SkillSmartTranslator,
    Reference(Domain),
    RoleCoordinator,
    RoleTranslator,
    RoleReviewer,
}

impl ResourceId {
    /// 解析外部 ID。`prompt:system` 与自由路径一律拒绝。
    pub fn parse(value: &str) -> Result<Self, ResourceError> {
        let value = value.trim();
        if value.is_empty() || value.len() > 128 {
            return Err(ResourceError::InvalidId(value.to_owned()));
        }
        if value.contains(['/', '\\', '\0']) || value.contains("..") {
            return Err(ResourceError::InvalidId(value.to_owned()));
        }
        if value.eq_ignore_ascii_case("prompt:system") || value.eq_ignore_ascii_case("system") {
            return Err(ResourceError::HostOnly("prompt:system".into()));
        }
        match value {
            "skill:smart-translator" => Ok(Self::SkillSmartTranslator),
            "role:coordinator" => Ok(Self::RoleCoordinator),
            "role:translator" => Ok(Self::RoleTranslator),
            "role:reviewer" => Ok(Self::RoleReviewer),
            _ => {
                if let Some(domain) = value.strip_prefix("reference:") {
                    return Domain::parse(domain)
                        .map(Self::Reference)
                        .ok_or_else(|| ResourceError::InvalidId(value.to_owned()));
                }
                Err(ResourceError::InvalidId(value.to_owned()))
            }
        }
    }

    /// 规范 ID 字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SkillSmartTranslator => "skill:smart-translator",
            Self::Reference(Domain::Literature) => "reference:domain-literature",
            Self::Reference(Domain::Acg) => "reference:domain-acg",
            Self::Reference(Domain::Academic) => "reference:domain-academic",
            Self::Reference(Domain::News) => "reference:domain-news",
            Self::Reference(Domain::Tech) => "reference:domain-tech",
            Self::Reference(Domain::Legal) => "reference:domain-legal",
            Self::RoleCoordinator => "role:coordinator",
            Self::RoleTranslator => "role:translator",
            Self::RoleReviewer => "role:reviewer",
        }
    }

    fn content(self) -> &'static str {
        match self {
            Self::SkillSmartTranslator => SKILL,
            Self::Reference(Domain::Literature) => REF_LITERATURE,
            Self::Reference(Domain::Acg) => REF_ACG,
            Self::Reference(Domain::Academic) => REF_ACADEMIC,
            Self::Reference(Domain::News) => REF_NEWS,
            Self::Reference(Domain::Tech) => REF_TECH,
            Self::Reference(Domain::Legal) => REF_LEGAL,
            Self::RoleCoordinator => AGENT_COORDINATOR,
            Self::RoleTranslator => AGENT_TRANSLATOR,
            Self::RoleReviewer => AGENT_REVIEWER,
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 资源加载错误。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResourceError {
    #[error("资源 ID 无效: {0}")]
    InvalidId(String),
    #[error("资源 {0} 只允许宿主读取，模型不可加载")]
    HostOnly(String),
}

/// 按 ID 加载资源内容。
pub fn load(id: ResourceId) -> &'static str {
    id.content()
}

/// 按字符串 ID 加载；语义等同 [`ResourceId::parse`] + [`load`]。
pub fn load_resource(value: &str) -> Result<&'static str, ResourceError> {
    ResourceId::parse(value).map(load)
}

/// 基础系统提示词；只供宿主读取，不进入工具可见的资源集合。
pub fn system_prompt() -> &'static str {
    SYSTEM_PROMPT
}

/// 角色定义（等价于 `role:*` 资源）。
pub fn role_prompt(role: Role) -> &'static str {
    match role {
        Role::Main => AGENT_COORDINATOR,
        Role::Translator => AGENT_TRANSLATOR,
        Role::Reviewer => AGENT_REVIEWER,
    }
}

/// 资源内容版本：每组内容按文件名排序后拼接的 SHA-256（十六进制）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVersions {
    /// `prompts/` 组。
    pub prompts: String,
    /// `agents/` 组。
    pub agents: String,
    /// `skills/smart-translator/` 组（SKILL + references）。
    pub skill: String,
}

impl ResourceVersions {
    /// 当前编译进二进制的资源版本，用于写入 manifest 并在恢复时比对。
    pub fn current() -> Self {
        Self {
            prompts: sha256_hex(&[SYSTEM_PROMPT]),
            agents: sha256_hex(&AGENTS_SORTED),
            skill: sha256_hex(&SKILL_SORTED),
        }
    }
}

/// `agents/`：coordinator.md / reviewer.md / translator.md，按文件名排序。
const AGENTS_SORTED: [&str; 3] = [AGENT_COORDINATOR, AGENT_REVIEWER, AGENT_TRANSLATOR];

/// `skills/smart-translator/`：SKILL.md + references/*.md，按文件名排序。
const SKILL_SORTED: [&str; 7] = [
    SKILL,
    REF_ACADEMIC,
    REF_ACG,
    REF_LEGAL,
    REF_LITERATURE,
    REF_NEWS,
    REF_TECH,
];

fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_frozen_id_set() {
        for id in [
            "skill:smart-translator",
            "reference:domain-literature",
            "reference:domain-acg",
            "reference:domain-academic",
            "reference:domain-news",
            "reference:domain-tech",
            "reference:domain-legal",
            "role:coordinator",
            "role:translator",
            "role:reviewer",
        ] {
            let parsed = ResourceId::parse(id).unwrap_or_else(|err| panic!("{id}: {err}"));
            assert_eq!(parsed.as_str(), id);
            assert!(!load(parsed).is_empty(), "{id} 内容不应为空");
        }
        for id in [
            "",
            "prompt:system",
            "system",
            "skill:other",
            "reference:domain-unknown",
            "role:main",
            "../../etc/passwd",
            "skill\\smart-translator",
            "C:/secret",
        ] {
            assert!(ResourceId::parse(id).is_err(), "{id} 必须拒绝");
        }
    }

    #[test]
    fn system_prompt_is_host_only() {
        assert!(!system_prompt().is_empty());
        assert_eq!(
            load_resource("prompt:system"),
            Err(ResourceError::HostOnly("prompt:system".into()))
        );
        assert!(load_resource("role:coordinator").is_ok());
    }

    #[test]
    fn versions_are_stable_hex_and_differ_per_group() {
        let versions = ResourceVersions::current();
        let again = ResourceVersions::current();
        assert_eq!(versions, again, "同一次编译内版本必须稳定");
        for hash in [&versions.prompts, &versions.agents, &versions.skill] {
            assert_eq!(hash.len(), 64, "SHA-256 十六进制必须是 64 位");
            assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert_ne!(versions.prompts, versions.agents);
        assert_ne!(versions.agents, versions.skill);
    }

    #[test]
    fn role_prompt_matches_role_resource() {
        for (role, id) in [
            (Role::Main, "role:coordinator"),
            (Role::Translator, "role:translator"),
            (Role::Reviewer, "role:reviewer"),
        ] {
            assert_eq!(role_prompt(role), load_resource(id).expect("角色资源"));
        }
    }
}
