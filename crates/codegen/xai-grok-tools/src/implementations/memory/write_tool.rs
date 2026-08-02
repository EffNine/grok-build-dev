//! `memory` tool — write operations for Hermes-compatible memory files.
//!
//! This tool allows the agent to write to `~/.hermes/memories/MEMORY.md`
//! and `~/.hermes/memories/USER.md` using the same `§` delimiter format
//! that Hermes uses. This enables cross-compatibility: grok writes
//! memories that Hermes can read, and vice versa.

use std::path::PathBuf;

use crate::util::grok_home::grok_home;
use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Registered name of the `memory` write tool.
pub const MEMORY_TOOL_NAME: &str = "memory";

/// Delimiter used to separate memory entries in MEMORY.md and USER.md.
const ENTRY_DELIMITER: &str = "§";

/// Maximum character limits for memory files (matching Hermes contract).
const MEMORY_MAX_CHARS: usize = 2_200;
const USER_MAX_CHARS: usize = 1_375;

/// Input for the `memory` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryInput {
    /// Action to perform: "add", "replace", or "remove".
    #[serde(rename = "action")]
    pub action: MemoryAction,
    /// Target file: "memory" → MEMORY.md, "user" → USER.md.
    #[serde(rename = "target")]
    pub target: MemoryTarget,
    /// Entry text to add or the unique substring to match for replace/remove.
    #[serde(rename = "entry")]
    pub entry: String,
    /// Replacement text (required for "replace" action).
    #[serde(rename = "new_text", default, skip_serializing_if = "Option::is_none")]
    pub new_text: Option<String>,
}

/// Actions supported by the memory tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryAction {
    /// Append a new entry to the memory file.
    Add,
    /// Replace an existing entry matching `entry` (unique substring match).
    Replace,
    /// Remove an existing entry matching `entry` (unique substring match).
    Remove,
}

/// Targets for memory writes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryTarget {
    /// Global MEMORY.md — shared preferences and project knowledge.
    Memory,
    /// USER.md — user-specific preferences and identity.
    User,
}

impl MemoryTarget {
    /// Returns the file name for this target.
    pub fn file_name(&self) -> &'static str {
        match self {
            MemoryTarget::Memory => "MEMORY.md",
            MemoryTarget::User => "USER.md",
        }
    }

    /// Returns the max character limit for this target.
    pub fn max_chars(&self) -> usize {
        match self {
            MemoryTarget::Memory => MEMORY_MAX_CHARS,
            MemoryTarget::User => USER_MAX_CHARS,
        }
    }

    /// Returns the path to the hermes-compatible memory directory.
    pub fn dir_path(&self) -> PathBuf {
        grok_home().join(".hermes").join("memories")
    }
}

#[derive(Debug, Default)]
pub struct MemoryWriteImpl;

impl crate::types::tool_metadata::ToolMetadata for MemoryWriteImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::MemorySearch
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Write to cross-session memory files in the Hermes-compatible format.\n\n\
         Use this tool to persist important facts, preferences, and learnings \
         across sessions. The memory files are stored at ~/.hermes/memories/ \
         so they can be read by both grok and Hermes CLI.\n\n\
         **When to use proactively:**\n\
         - User states a preference (\"I prefer X over Y\") → target: user\n\
         - Environment fact discovered (OS, tool versions, project structure) → target: memory\n\
         - User corrects your approach → target: memory\n\
         - Project convention discovered (lint rules, test commands, style) → target: memory\n\
         - Explicit \"remember that...\" request → target: memory\n\
         - Skip: trivial/vague facts, easily re-derivable facts, raw data dumps\n\n\
         **Actions:**\n\
         - `add`: Append a new entry. Rejects exact duplicates silently.\n\
         - `replace`: Replace an existing entry by unique substring match.\n\
         - `remove`: Remove an existing entry by unique substring match.\n\n\
         **Format:** Each entry is separated by § delimiter. Max 2,200 chars \
         for MEMORY.md, 1,375 chars for USER.md."
    }
}

impl xai_tool_runtime::Tool for MemoryWriteImpl {
    type Args = MemoryInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("memory").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "memory",
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
        input: MemoryInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let target = input.target;
        let dir = target.dir_path();
        let file_name = target.file_name();
        let file_path = dir.join(file_name);
        let max_chars = target.max_chars();

        // Ensure directory exists
        std::fs::create_dir_all(&dir).map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                self.id(),
                format!("failed to create memory dir: {e}"),
            )
        })?;

        // Read existing content
        let content = std::fs::read_to_string(&file_path).unwrap_or_default();
        let entries = parse_entries(&content);

        match input.action {
            MemoryAction::Add => handle_add(&entries, &input.entry, max_chars, &file_path),
            MemoryAction::Replace => {
                handle_replace(&entries, &input.entry, input.new_text.as_deref(), max_chars, &file_path)
            }
            MemoryAction::Remove => handle_remove(&entries, &input.entry, &file_path),
        }
    }
}

/// Parse entries from a memory file (split by § delimiter).
fn parse_entries(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .split(ENTRY_DELIMITER)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Format entries back into file content.
fn format_entries(entries: &[String]) -> String {
    entries
        .iter()
        .filter(|e| !e.is_empty())
        .map(|e| format!("{ENTRY_DELIMITER} {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Handle the "add" action.
fn handle_add(
    entries: &[String],
    entry: &str,
    max_chars: usize,
    file_path: &std::path::Path,
) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
    // Check for exact duplicate
    if entries.contains(&entry.to_string()) {
        return Ok(ToolOutput::Text(
            "No duplicate added — entry already exists.".into(),
        ));
    }

    // Check char limit
    let new_content = format_entries(entries) + &format!("\n{ENTRY_DELIMITER} {entry}");
    if new_content.len() > max_chars {
        return Ok(ToolOutput::Text(format!(
            "Memory file would exceed {}-char limit (current: {} chars, new entry: {} chars). \
             Please consolidate existing entries via `replace` before adding more.",
            max_chars,
            content_len(file_path),
            entry.len()
        ).into()));
    }

    // Append entry
    let new_entries = if entries.is_empty() {
        vec![entry.to_string()]
    } else {
        let mut e = entries.to_vec();
        e.push(entry.to_string());
        e
    };
    let formatted = format_entries(&new_entries);
    std::fs::write(file_path, formatted).map_err(|e| {
        xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!("failed to write memory: {e}"),
        )
    })?;

    Ok(ToolOutput::Text("Entry added to memory.".into()))
}

/// Handle the "replace" action.
fn handle_replace(
    entries: &[String],
    old_text: &str,
    new_text: Option<&str>,
    max_chars: usize,
    file_path: &std::path::Path,
) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
    let matches: Vec<&String> = entries
        .iter()
        .filter(|e| e.contains(old_text))
        .collect();

    if matches.is_empty() {
        return Err(xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!("No entry matches \"{old_text}\""),
        ));
    }
    if matches.len() > 1 {
        return Err(xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!(
                "Multiple entries match \"{old_text}\" — be more specific. Matches:\n{}",
                matches
                    .iter()
                    .map(|e| format!("  - {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        ));
    }

    let new_entry = new_text.unwrap_or(old_text).to_string();

    // Check char limit
    let mut new_entries: Vec<String> = entries.to_vec();
    let pos = new_entries
        .iter()
        .position(|e| e.contains(old_text))
        .unwrap();
    new_entries[pos] = new_entry;
    let formatted = format_entries(&new_entries);
    if formatted.len() > max_chars {
        return Ok(ToolOutput::Text(format!(
            "Memory file would exceed {}-char limit after replacement (current: {} chars, new content: {} chars). \
             Please consolidate via additional `replace` calls.",
            max_chars,
            content_len(file_path),
            formatted.len()
        ).into()));
    }

    std::fs::write(file_path, formatted).map_err(|e| {
        xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!("failed to write memory: {e}"),
        )
    })?;

    Ok(ToolOutput::Text("Entry replaced in memory.".into()))
}

/// Handle the "remove" action.
fn handle_remove(
    entries: &[String],
    old_text: &str,
    file_path: &std::path::Path,
) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
    let matches: Vec<&String> = entries
        .iter()
        .filter(|e| e.contains(old_text))
        .collect();

    if matches.is_empty() {
        return Err(xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!("No entry matches \"{old_text}\""),
        ));
    }
    if matches.len() > 1 {
        return Err(xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!(
                "Multiple entries match \"{old_text}\" — be more specific. Matches:\n{}",
                matches
                    .iter()
                    .map(|e| format!("  - {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        ));
    }

    let mut new_entries: Vec<String> = entries.to_vec();
    new_entries.retain(|e| !e.contains(old_text));
    let formatted = format_entries(&new_entries);
    std::fs::write(file_path, formatted).map_err(|e| {
        xai_tool_runtime::ToolError::execution(
            xai_tool_protocol::ToolId::new("memory").expect("valid"),
            format!("failed to write memory: {e}"),
        )
    })?;

    Ok(ToolOutput::Text("Entry removed from memory.".into()))
}

/// Get current content length of a file.
fn content_len(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path).map(|s| s.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_entries_empty() {
        assert!(parse_entries("").is_empty());
    }

    #[test]
    fn test_parse_entries_single() {
        let entries = parse_entries("§ first entry");
        assert_eq!(entries, vec!["first entry"]);
    }

    #[test]
    fn test_parse_entries_multiple() {
        let entries = parse_entries("§ first\n§ second\n§ third");
        assert_eq!(entries, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_format_entries_empty() {
        assert_eq!(format_entries(&[]), "");
    }

    #[test]
    fn test_format_entries_multiple() {
        let result = format_entries(&["first".to_string(), "second".to_string()]);
        assert!(result.contains("first"));
        assert!(result.contains("second"));
        assert!(result.contains(ENTRY_DELIMITER));
    }

    #[test]
    fn test_memory_target_paths() {
        let memory = MemoryTarget::Memory;
        assert_eq!(memory.file_name(), "MEMORY.md");
        assert_eq!(memory.max_chars(), MEMORY_MAX_CHARS);
        assert!(memory.dir_path().to_string_lossy().contains(".hermes"));

        let user = MemoryTarget::User;
        assert_eq!(user.file_name(), "USER.md");
        assert_eq!(user.max_chars(), USER_MAX_CHARS);
    }
}
