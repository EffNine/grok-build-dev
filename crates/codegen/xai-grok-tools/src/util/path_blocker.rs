//! Unified path exclusion for tools (`blocked_paths` / `GROK_BLOCKED_PATHS`).

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Default blocked path globs applied when the user does not override
/// `[tools] blocked_paths` or `GROK_BLOCKED_PATHS`.
pub const DEFAULT_BLOCKED_PATHS: &[&str] = &[
    "node_modules/**",
    ".git/**",
    "target/**",
    "build/**",
    "dist/**",
    "*.pyc",
    "__pycache__/**",
];

/// Shared path blocker checked by Read / Grep / Glob / Edit / Bash tools.
///
/// Patterns are matched against the path relative to the session CWD (when
/// the path is under CWD) and also against the full path with `/` separators.
#[derive(Clone, Debug)]
pub struct PathBlocker {
    patterns: Vec<String>,
    set: GlobSet,
}

impl Default for PathBlocker {
    fn default() -> Self {
        Self::from_patterns(DEFAULT_BLOCKED_PATHS.iter().map(|s| (*s).to_string()))
    }
}

impl PathBlocker {
    /// Build a blocker from an ordered list of glob patterns.
    ///
    /// Invalid patterns are skipped (with a warning at the call site if
    /// desired); an empty list yields a no-op blocker that never matches.
    pub fn from_patterns(patterns: impl IntoIterator<Item = String>) -> Self {
        let mut builder = GlobSetBuilder::new();
        let mut kept = Vec::new();
        for pat in patterns {
            let trimmed = pat.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            for candidate in expanded_patterns(&trimmed) {
                if let Ok(glob) = build_glob(&candidate) {
                    builder.add(glob);
                }
            }
            kept.push(trimmed);
        }
        let set = builder.build().unwrap_or_else(|_| GlobSet::empty());
        Self {
            patterns: kept,
            set,
        }
    }

    /// Patterns currently in effect (for diagnostics / `/config` display).
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// True when `path` matches any blocked glob.
    ///
    /// `cwd` is used to relativize absolute paths under the session root so
    /// patterns like `target/**` match `/proj/target/foo`.
    pub fn is_blocked(&self, path: &Path, cwd: Option<&Path>) -> bool {
        if self.set.is_empty() {
            return false;
        }
        for candidate in path_candidates(path, cwd) {
            if self.set.is_match(&candidate) {
                return true;
            }
        }
        false
    }

    /// True when any path-like token in `command` is blocked.
    ///
    /// Tokens are whitespace/quote-split fragments that look like paths
    /// (contain `/`, start with `./`/`../`, or end with a blocked extension).
    pub fn command_references_blocked(&self, command: &str, cwd: Option<&Path>) -> Option<String> {
        if self.set.is_empty() {
            return None;
        }
        for token in command_path_tokens(command) {
            let path = Path::new(token);
            if self.is_blocked(path, cwd) {
                return Some(self.blocked_message(path));
            }
        }
        None
    }

    /// Human-readable error for a blocked path.
    pub fn blocked_message(&self, display_path: &Path) -> String {
        format!(
            "Error: {} is blocked by tools.blocked_paths and cannot be accessed.",
            display_path.display()
        )
    }
}

fn command_path_tokens(command: &str) -> Vec<&str> {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ';' | '|' | '&' | '`' | '(' | ')' | '<' | '>'))
        .filter(|t| {
            let t = t.trim_matches(|c| matches!(c, '"' | '\'' | ','));
            !t.is_empty()
                && (t.contains('/')
                    || t.starts_with('.')
                    || t.ends_with(".pyc")
                    || t == "node_modules"
                    || t == "target"
                    || t == "build"
                    || t == "dist"
                    || t == "__pycache__"
                    || t == ".git")
        })
        .collect()
}

fn build_glob(pattern: &str) -> Result<Glob, globset::Error> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
}

fn expanded_patterns(pattern: &str) -> Vec<String> {
    let mut out = vec![pattern.to_string()];
    if !pattern.starts_with("**/") && !pattern.starts_with('/') {
        out.push(format!("**/{pattern}"));
    }
    // Also match the bare directory / file stem so a walk entry named
    // `node_modules` is blocked even before descending into it.
    if let Some(base) = pattern.strip_suffix("/**") {
        out.push(base.to_string());
        if !base.starts_with("**/") {
            out.push(format!("**/{base}"));
        }
    } else if let Some(base) = pattern.strip_suffix('/') {
        out.push(base.to_string());
    }
    out
}

fn path_to_unix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_candidates(path: &Path, cwd: Option<&Path>) -> Vec<String> {
    let mut out = Vec::new();
    let absolute = path_to_unix(path);
    if !absolute.is_empty() {
        out.push(absolute.trim_start_matches('/').to_string());
        out.push(absolute);
    }
    if let Some(cwd) = cwd
        && let Ok(rel) = path.strip_prefix(cwd)
    {
        let rel = path_to_unix(rel);
        if !rel.is_empty() {
            out.push(rel);
        }
    } else if !path.is_absolute() {
        out.push(path_to_unix(path));
    }
    // File name alone (covers `*.pyc` style patterns).
    if let Some(name) = path.file_name() {
        out.push(name.to_string_lossy().into_owned());
    }
    // Every path suffix so `pkg/node_modules/foo` also matches via
    // intermediate `node_modules/foo` / `node_modules` candidates.
    let unix = path_to_unix(path);
    let parts: Vec<&str> = unix.split('/').filter(|p| !p.is_empty()).collect();
    for i in 0..parts.len() {
        out.push(parts[i..].join("/"));
    }
    out.sort();
    out.dedup();
    out
}

/// Parse `GROK_BLOCKED_PATHS` (comma or colon separated). Empty string clears
/// defaults when the caller uses the returned `Some(vec![])` as an override.
pub fn parse_blocked_paths_env(raw: &str) -> Vec<String> {
    raw.split([',', ':'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn defaults_block_node_modules_and_target() {
        let blocker = PathBlocker::default();
        let cwd = PathBuf::from("/proj");
        assert!(blocker.is_blocked(Path::new("/proj/node_modules/lodash/index.js"), Some(&cwd)));
        assert!(blocker.is_blocked(Path::new("/proj/target/debug/foo"), Some(&cwd)));
        assert!(blocker.is_blocked(Path::new("/proj/.git/config"), Some(&cwd)));
        assert!(blocker.is_blocked(Path::new("/proj/src/foo.pyc"), Some(&cwd)));
        assert!(!blocker.is_blocked(Path::new("/proj/src/main.rs"), Some(&cwd)));
    }

    #[test]
    fn empty_blocker_allows_all() {
        let blocker = PathBlocker::from_patterns(std::iter::empty());
        assert!(!blocker.is_blocked(Path::new("/proj/node_modules/x"), None));
    }

    #[test]
    fn relative_paths_match() {
        let blocker = PathBlocker::default();
        assert!(blocker.is_blocked(Path::new("node_modules/pkg"), Some(Path::new("/proj"))));
        assert!(blocker.is_blocked(Path::new("build/out.js"), None));
    }

    #[test]
    fn parse_env() {
        assert_eq!(
            parse_blocked_paths_env("a/**, b/**:c/**"),
            vec!["a/**", "b/**", "c/**"]
        );
        assert!(parse_blocked_paths_env("").is_empty());
    }
}
