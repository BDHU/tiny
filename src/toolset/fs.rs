use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use globset::{GlobBuilder, GlobMatcher};
use regex::RegexBuilder;
use schemars::JsonSchema;
use serde::Deserialize;
use tiny::Tool;
use tokio::fs;

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Path to the file.
    path: String,
}

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    type Args = ReadArgs;

    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from disk."
    }

    async fn call(&self, args: ReadArgs) -> Result<String> {
        Ok(fs::read_to_string(args.path).await?)
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// Path to the file.
    path: String,
    /// Content to write.
    content: String,
}

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    type Args = WriteArgs;

    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file, overwriting any existing file at the path."
    }

    async fn call(&self, args: WriteArgs) -> Result<String> {
        let bytes = args.content.len();
        fs::write(&args.path, args.content).await?;
        Ok(format!("wrote {bytes} bytes to {}", args.path))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct EditArgs {
    /// Path to the file.
    path: String,
    /// Exact text to replace. It must appear exactly once.
    old_string: String,
    /// Replacement text.
    new_string: String,
}

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    type Args = EditArgs;

    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file. The old string must appear exactly once."
    }

    async fn call(&self, args: EditArgs) -> Result<String> {
        if args.old_string.is_empty() {
            return Err(anyhow!("old_string must not be empty"));
        }

        let original = fs::read_to_string(&args.path).await?;
        let count = original.matches(&args.old_string).count();
        if count == 0 {
            return Err(anyhow!("old_string not found in {}", args.path));
        }
        if count > 1 {
            return Err(anyhow!(
                "old_string appears {count} times in {}; provide more context to make it unique",
                args.path
            ));
        }

        let updated = original.replacen(&args.old_string, &args.new_string, 1);
        fs::write(&args.path, updated).await?;
        Ok(format!("replaced 1 occurrence in {}", args.path))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Directory path. Defaults to the current directory.
    path: Option<String>,
}

pub struct ListTool;

#[async_trait]
impl Tool for ListTool {
    type Args = ListArgs;

    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List the entries of a directory."
    }

    async fn call(&self, args: ListArgs) -> Result<String> {
        let path = args.path.as_deref().unwrap_or(".");
        let mut read_dir = fs::read_dir(path).await?;
        let mut entries = Vec::new();

        while let Some(entry) = read_dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let suffix = if entry.file_type().await?.is_dir() {
                "/"
            } else {
                ""
            };
            entries.push(format!("{name}{suffix}"));
        }

        entries.sort();
        Ok(entries.join("\n"))
    }
}

const DEFAULT_RESULT_LIMIT: usize = 200;

fn root_path(path: Option<String>) -> PathBuf {
    path.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn compile_file_glob(pattern: &str) -> Result<GlobMatcher> {
    let normalized = if pattern.contains('/') {
        pattern.to_string()
    } else {
        format!("**/{pattern}")
    };
    Ok(GlobBuilder::new(&normalized)
        .literal_separator(true)
        .build()?
        .compile_matcher())
}

fn walk_file_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = ignore::WalkBuilder::new(root)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path().to_path_buf())
        .collect();
    paths.sort();
    paths
}

fn relative_path<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

fn modified_time(path: &Path) -> SystemTime {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn render_limited(lines: Vec<String>, label: &str, limit: usize) -> String {
    if lines.is_empty() {
        return "(no matches)".to_string();
    }
    let total = lines.len();
    if total <= limit {
        return lines.join("\n");
    }
    let mut out: Vec<String> = lines.into_iter().take(limit).collect();
    out.push(format!(
        "... and {} more {label} (raise `limit` to see more)",
        total - limit
    ));
    out.join("\n")
}

#[derive(Deserialize, JsonSchema)]
pub struct GlobArgs {
    /// Glob pattern. Bare patterns like "*.rs" match at any depth; use "/" to anchor (e.g. "src/**/*.rs").
    pattern: String,
    /// Directory to search. Defaults to the current directory.
    path: Option<String>,
    /// Maximum results to return. Defaults to 200.
    limit: Option<usize>,
}

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    type Args = GlobArgs;

    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Respects .gitignore. Sorted newest-first by mtime."
    }

    async fn call(&self, args: GlobArgs) -> Result<String> {
        tokio::task::spawn_blocking(move || {
            let root = root_path(args.path);
            let limit = args.limit.unwrap_or(DEFAULT_RESULT_LIMIT);
            let matcher = compile_file_glob(&args.pattern)?;

            let mut hits: Vec<(SystemTime, String)> = walk_file_paths(&root)
                .into_iter()
                .filter(|path| matcher.is_match(relative_path(&root, path)))
                .map(|path| (modified_time(&path), path.display().to_string()))
                .collect();

            hits.sort_by_key(|hit| std::cmp::Reverse(hit.0));
            Ok(render_limited(
                hits.into_iter().map(|(_, p)| p).collect(),
                "files",
                limit,
            ))
        })
        .await?
    }
}

#[derive(Clone, Copy, Deserialize, JsonSchema, Default, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    Content,
    FilesWithMatches,
    Count,
}

impl OutputMode {
    fn result_label(self) -> &'static str {
        match self {
            OutputMode::Content => "matches",
            OutputMode::FilesWithMatches | OutputMode::Count => "files",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct GrepArgs {
    /// Regex pattern (Rust regex syntax).
    pattern: String,
    /// Directory to search. Defaults to the current directory.
    path: Option<String>,
    /// Optional file glob filter, e.g. "*.rs".
    glob: Option<String>,
    /// Case-insensitive match.
    #[serde(default)]
    case_insensitive: bool,
    /// content (default; file:line:text), files_with_matches, or count.
    #[serde(default)]
    output_mode: OutputMode,
    /// Maximum results to return. Defaults to 200.
    limit: Option<usize>,
}

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    type Args = GrepArgs;

    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with a regex. Respects .gitignore. Optional glob filter; three output modes."
    }

    async fn call(&self, args: GrepArgs) -> Result<String> {
        tokio::task::spawn_blocking(move || {
            let root = root_path(args.path);
            let limit = args.limit.unwrap_or(DEFAULT_RESULT_LIMIT);
            let regex = RegexBuilder::new(&args.pattern)
                .case_insensitive(args.case_insensitive)
                .build()?;
            let glob = args.glob.as_deref().map(compile_file_glob).transpose()?;

            let mut results = Vec::<String>::new();

            for path in walk_file_paths(&root) {
                if glob
                    .as_ref()
                    .is_some_and(|matcher| !matcher.is_match(relative_path(&root, &path)))
                {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let path_str = path.display().to_string();
                let mut hits = 0usize;
                for (i, line) in text.lines().enumerate() {
                    if regex.is_match(line) {
                        hits += 1;
                        if args.output_mode == OutputMode::Content {
                            results.push(format!("{path_str}:{}:{line}", i + 1));
                        }
                    }
                }

                match args.output_mode {
                    OutputMode::FilesWithMatches if hits > 0 => results.push(path_str),
                    OutputMode::Count if hits > 0 => results.push(format!("{path_str}:{hits}")),
                    OutputMode::Content | OutputMode::FilesWithMatches | OutputMode::Count => {}
                }
            }

            Ok(render_limited(
                results,
                args.output_mode.result_label(),
                limit,
            ))
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_glob_matches_at_any_depth() {
        let matcher = compile_file_glob("*.rs").unwrap();

        assert!(matcher.is_match(Path::new("src/main.rs")));
        assert!(matcher.is_match(Path::new("main.rs")));
        assert!(!matcher.is_match(Path::new("src/main.toml")));
    }

    #[test]
    fn slash_glob_keeps_path_context() {
        let matcher = compile_file_glob("src/**/*.rs").unwrap();

        assert!(matcher.is_match(Path::new("src/main.rs")));
        assert!(!matcher.is_match(Path::new("tests/main.rs")));
    }

    #[test]
    fn render_truncates_with_remaining_count() {
        let lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        assert_eq!(
            render_limited(lines, "matches", 2),
            "a\nb\n... and 1 more matches (raise `limit` to see more)"
        );
    }
}
