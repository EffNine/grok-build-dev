//! `skill_manage` tool — manage Hermes-compatible skills on disk.
//!
//! This tool allows the agent to create, edit, patch, and delete skills
//! in the Hermes-compatible format at `~/.hermes/skills/`. This enables
//! cross-compatibility: grok-authored skills can be read by Hermes CLI
//! and vice versa.
//!
//! NOTE: unlike Hermes, we don't scan agent-authored skills for dangerous
//! patterns before persisting — acceptable for solo/local use, revisit
//! if this store is ever shared beyond one machine.

use crate::util::grok_home::grok_home;
use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Registered name of the `skill_manage` tool.
pub const SKILL_MANAGE_TOOL_NAME: &str = "skill_manage";

/// Minimum required sections in a SKILL.md file.
const REQUIRED_SECTIONS: &[&str] = &["## When to Use", "## Procedure"];
const REQUIRED_FRONTMATTER: &[&str] = &["name:", "description:"];

#[derive(Debug, Default)]
pub struct SkillManageImpl;

impl crate::types::tool_metadata::ToolMetadata for SkillManageImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::Skill
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Manage Hermes-compatible skills on disk.\n\n\
         Use this tool to create, edit, patch, delete, or manage files for \
         skills stored at ~/.hermes/skills/. Skills are Markdown files that \
         grok and Hermes can both read.\n\n\
         **When to use proactively:**\n\
         - After completing a complex task (5+ tool calls / multi-step)\n\
         - After hitting errors and finding a working path\n\
         - After user corrects your approach mid-task\n\
         - After discovering a non-trivial, reusable workflow\n\n\
         **Actions:**\n\
         - `create`: New skill from scratch\n\
         - `patch`: Targeted fix with old_string → new_string\n\
         - `edit`: Full content replacement\n\
         - `delete`: Remove skill entirely\n\
         - `write_file`: Add/overwrite supporting file\n\
         - `remove_file`: Delete supporting file\n\n\
         **Required SKILL.md format:**\n\
         ```\n\
         ---\n\
         name: skill-name\n\
         description: <= 60 chars, imperative, specific\n\
         version: 1.0.0\n\
         ---\n\
         # Skill Title\n\
         ## When to Use\n\
         ## Procedure\n\
         ## Pitfalls\n\
         ## Verification\n\
         ```\n\n\
         Skills missing required frontmatter or sections will be rejected."
    }
}

impl xai_tool_runtime::Tool for SkillManageImpl {
    type Args = SkillManageInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("skill_manage").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "skill_manage",
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        _ctx: xai_tool_runtime::ToolCallContext,
        input: SkillManageInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let skills_dir = grok_home().join(".hermes").join("skills");
        let skill_dir = skills_dir.join(&input.name);

        match input.action {
            SkillAction::Create => {
                let content = input.content.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "create action requires 'content' field".to_string(),
                    )
                })?;
                validate_skill_content(content)?;
                std::fs::create_dir_all(&skill_dir).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to create skill dir: {e}"),
                    )
                })?;
                if let Some(category) = &input.category {
                    let category_dir = skill_dir.join(category);
                    std::fs::create_dir_all(&category_dir).map_err(|e| {
                        xai_tool_runtime::ToolError::execution(
                            self.id(),
                            format!("failed to create category dir: {e}"),
                        )
                    })?;
                }
                std::fs::write(skill_dir.join("SKILL.md"), content).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to write SKILL.md: {e}"),
                    )
                })?;
                Ok(ToolOutput::Text(format!("Created skill '{}'.", input.name).into()))
            }
            SkillAction::Patch => {
                let old_string = input.old_string.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "patch action requires 'old_string' field".to_string(),
                    )
                })?;
                let new_string = input.new_string.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "patch action requires 'new_string' field".to_string(),
                    )
                })?;
                let skill_md = skill_dir.join("SKILL.md");
                let content = std::fs::read_to_string(&skill_md).map_err(|_| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("skill '{}' not found", input.name),
                    )
                })?;
                if !content.contains(old_string.as_str()) {
                    return Err(xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!(
                            "old_string not found in skill '{}': {}",
                            input.name, old_string
                        ),
                    ));
                }
                let new_content = content.replace(old_string, new_string);
                validate_skill_content(&new_content)?;
                std::fs::write(&skill_md, new_content).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to write SKILL.md: {e}"),
                    )
                })?;
                Ok(ToolOutput::Text(format!(
                    "Patched skill '{}'.",
                    input.name
                ).into()))
            }
            SkillAction::Edit => {
                let content = input.content.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "edit action requires 'content' field".to_string(),
                    )
                })?;
                validate_skill_content(content)?;
                let skill_md = skill_dir.join("SKILL.md");
                if !skill_md.exists() {
                    return Err(xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("skill '{}' not found", input.name),
                    ));
                }
                std::fs::write(&skill_md, content).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to write SKILL.md: {e}"),
                    )
                })?;
                Ok(ToolOutput::Text(format!(
                    "Edited skill '{}'.",
                    input.name
                ).into()))
            }
            SkillAction::Delete => {
                if !skill_dir.exists() {
                    return Err(xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("skill '{}' not found", input.name),
                    ));
                }
                std::fs::remove_dir_all(&skill_dir).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to delete skill: {e}"),
                    )
                })?;
                Ok(ToolOutput::Text(format!(
                    "Deleted skill '{}'.",
                    input.name
                ).into()))
            }
            SkillAction::WriteFile => {
                let file_path = input.file_path.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "write_file action requires 'file_path' field".to_string(),
                    )
                })?;
                let content = input.content.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "write_file action requires 'content' field".to_string(),
                    )
                })?;
                let target_path = skill_dir.join(file_path);
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        xai_tool_runtime::ToolError::execution(
                            self.id(),
                            format!("failed to create dir: {e}"),
                        )
                    })?;
                }
                std::fs::write(&target_path, content).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to write file: {e}"),
                    )
                })?;
                Ok(ToolOutput::Text(format!(
                    "Wrote file '{}' in skill '{}'.",
                    file_path, input.name
                ).into()))
            }
            SkillAction::RemoveFile => {
                let file_path = input.file_path.as_ref().ok_or_else(|| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        "remove_file action requires 'file_path' field".to_string(),
                    )
                })?;
                let target_path = skill_dir.join(file_path);
                if !target_path.exists() {
                    return Err(xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("file '{}' not found in skill '{}'", file_path, input.name),
                    ));
                }
                std::fs::remove_file(&target_path).map_err(|e| {
                    xai_tool_runtime::ToolError::execution(
                        self.id(),
                        format!("failed to remove file: {e}"),
                    )
                })?;
                Ok(ToolOutput::Text(format!(
                    "Removed file '{}' from skill '{}'.",
                    file_path, input.name
                ).into()))
            }
        }
    }
}

/// Input for the `skill_manage` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SkillManageInput {
    /// The name of the skill to manage (matches directory name under ~/.hermes/skills/).
    #[serde(rename = "name")]
    pub name: String,
    /// Action to perform.
    #[serde(rename = "action")]
    pub action: SkillAction,
    /// Full SKILL.md content (required for create, edit, patch).
    #[serde(rename = "content", default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional category subdirectory (e.g., "backend", "frontend").
    #[serde(rename = "category", default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// For patch: the exact string to find and replace.
    #[serde(rename = "old_string", default, skip_serializing_if = "Option::is_none")]
    pub old_string: Option<String>,
    /// For patch: the replacement string.
    #[serde(rename = "new_string", default, skip_serializing_if = "Option::is_none")]
    pub new_string: Option<String>,
    /// For write_file/remove_file: the relative path within the skill directory.
    #[serde(rename = "file_path", default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Actions supported by the skill_manage tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SkillAction {
    /// Create a new skill from scratch.
    Create,
    /// Targeted fix: find old_string and replace with new_string.
    Patch,
    /// Full replacement of SKILL.md content.
    Edit,
    /// Delete the entire skill directory.
    Delete,
    /// Write or overwrite a supporting file within the skill.
    WriteFile,
    /// Remove a supporting file from the skill.
    RemoveFile,
}

/// Validate that skill content has required frontmatter and sections.
fn validate_skill_content(content: &str) -> Result<(), xai_tool_runtime::ToolError> {
    // Check frontmatter
    for field in REQUIRED_FRONTMATTER {
        if !content.contains(field) {
            return Err(xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("skill_manage").expect("valid"),
                format!(
                    "SKILL.md missing required frontmatter field: '{}'. \
                     Add '---\\n{}: ...\\n---' at the top of the file.",
                    field, field
                ),
            ));
        }
    }

    // Check sections
    for section in REQUIRED_SECTIONS {
        if !content.contains(section) {
            return Err(xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("skill_manage").expect("valid"),
                format!(
                    "SKILL.md missing required section: '{}'. \
                     Add a '#{}' heading to the file.",
                    section, section
                ),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_skill() {
        let content = r#"---
name: test-skill
description: A test skill
version: 1.0.0
---
# Test Skill
## When to Use
When you need to test.
## Procedure
1. Do something
## Pitfalls
Watch out for X.
## Verification
Check Y.
"#;
        assert!(validate_skill_content(content).is_ok());
    }

    #[test]
    fn test_validate_missing_frontmatter() {
        let content = "# No frontmatter\n## When to Use\nTest\n## Procedure\nTest";
        assert!(validate_skill_content(content).is_err());
    }

    #[test]
    fn test_validate_missing_section() {
        let content = r#"---
name: test
description: test
---
# Test
## When to Use
Test"#;
        assert!(validate_skill_content(content).is_err());
    }

    #[test]
    fn test_tool_name_constant() {
        assert_eq!(SKILL_MANAGE_TOOL_NAME, "skill_manage");
    }
}
