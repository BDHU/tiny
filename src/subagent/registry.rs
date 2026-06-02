use anyhow::{anyhow, Result};

#[derive(Clone, Copy, Debug)]
pub(crate) enum ToolsetKind {
    ReadOnly,
}

#[derive(Debug)]
pub(crate) struct SubagentSpec {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) system: &'static str,
    pub(crate) toolset: ToolsetKind,
}

const REVIEWER_SYSTEM: &str = r#"You are a read-only code review subagent.

Focus on bugs, behavioral regressions, unclear edge cases, and missing tests.
Do not make code changes. Do not propose broad refactors unless they prevent a concrete bug.
Use concise findings with file paths and line numbers when available.

Return exactly these sections:

Findings
- List issues by severity, or "(none)".

Files Inspected
- List files you read or searched.

Notes
- Mention test gaps, assumptions, or "(none)"."#;

const BUILTIN_SUBAGENTS: &[SubagentSpec] = &[SubagentSpec {
    name: "reviewer",
    description: "Read-only code reviewer for bugs, regressions, and test gaps.",
    system: REVIEWER_SYSTEM,
    toolset: ToolsetKind::ReadOnly,
}];

pub(crate) fn find(name: &str) -> Result<&'static SubagentSpec> {
    let normalized = name.trim();
    BUILTIN_SUBAGENTS
        .iter()
        .find(|spec| spec.name == normalized)
        .ok_or_else(|| {
            anyhow!(
                "unknown subagent '{normalized}'. Available subagents: {}",
                names().join(", ")
            )
        })
}

pub(crate) fn delegate_description() -> &'static str {
    "Run a focused built-in subagent and return its concise report. Available agents: reviewer."
}

fn names() -> Vec<&'static str> {
    BUILTIN_SUBAGENTS.iter().map(|spec| spec.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_builtin_reviewer() {
        let spec = find("reviewer").expect("reviewer exists");
        assert_eq!(spec.name, "reviewer");
    }

    #[test]
    fn rejects_unknown_subagent() {
        let error = find("writer").unwrap_err().to_string();
        assert!(error.contains("unknown subagent"));
        assert!(error.contains("reviewer"));
    }
}
