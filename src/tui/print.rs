use crate::tui::theme;
use crossterm::{
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal,
    terminal::{Clear, ClearType},
};
use serde_json::Value;
use std::io::{self, Write};
use tiny::Message;

const NL: &str = "\r\n";
const COLLAPSED_TOOL_RESULT_LINES: usize = 3;

pub(crate) enum Entry {
    User(String),
    Assistant(String),
    ToolCall { name: String, args: Value },
    ToolResult { content: String, is_error: bool },
    Error(String),
}

pub(crate) fn entries_from_message(message: Message) -> Vec<Entry> {
    match message {
        Message::User(text) => vec![Entry::User(text)],
        Message::Assistant { text, tool_calls } => {
            let mut out = Vec::new();
            if !text.is_empty() {
                out.push(Entry::Assistant(text));
            }
            out.extend(tool_calls.into_iter().map(|call| Entry::ToolCall {
                name: call.name,
                args: call.input,
            }));
            out
        }
        Message::Tool(result) => vec![Entry::ToolResult {
            content: result.content,
            is_error: result.is_error,
        }],
    }
}

pub(crate) fn print_entry<W: Write>(out: &mut W, entry: &Entry) -> io::Result<()> {
    match entry {
        Entry::User(text) => print_user(out, text),
        Entry::Assistant(text) => print_assistant(out, text),
        Entry::ToolCall { name, args } => print_tool_call(out, name, args),
        Entry::ToolResult { content, is_error } => print_tool_result(out, content, *is_error),
        Entry::Error(text) => print_error(out, text),
    }
}

pub(crate) fn print_user<W: Write>(out: &mut W, text: &str) -> io::Result<()> {
    queue!(out, Print(NL))?;
    for (i, line) in text.split('\n').enumerate() {
        let prefix = if i == 0 { "> " } else { "  " };
        queue!(
            out,
            SetBackgroundColor(theme::USER_BG),
            Print(theme::GUTTER),
            SetForegroundColor(theme::DIM),
            Print(prefix),
            ResetColor,
            SetBackgroundColor(theme::USER_BG),
            Print(line),
            Clear(ClearType::UntilNewLine),
            ResetColor,
            Print(NL),
        )?;
    }
    Ok(())
}

pub(crate) fn print_assistant<W: Write>(out: &mut W, text: &str) -> io::Result<()> {
    queue!(out, Print(NL))?;
    let mut in_code = false;
    let mut first = true;

    for raw in text.split('\n') {
        let trimmed = raw.trim_start();
        let is_fence = trimmed.starts_with("```");
        let raw_code_line = is_fence || in_code;

        queue!(out, Print(theme::GUTTER))?;
        if first {
            queue!(
                out,
                SetForegroundColor(theme::ASSISTANT),
                SetAttribute(Attribute::Bold),
                Print("· "),
                SetAttribute(Attribute::NormalIntensity),
                ResetColor,
            )?;
            first = false;
        } else {
            queue!(out, Print("  "))?;
        }

        if raw_code_line {
            queue!(out, SetForegroundColor(theme::DIM), Print(raw), ResetColor)?;
            if is_fence {
                in_code = !in_code;
            }
        } else if let Some(rest) = heading_body(raw) {
            queue!(
                out,
                SetAttribute(Attribute::Bold),
                Print(rest),
                SetAttribute(Attribute::NormalIntensity),
            )?;
        } else if let Some(rest) = bullet_body(raw) {
            queue!(out, Print("• "))?;
            print_inline_code(out, rest)?;
        } else {
            print_inline_code(out, raw)?;
        }

        queue!(out, Print(NL))?;
    }
    Ok(())
}

pub(crate) fn print_tool_call<W: Write>(out: &mut W, name: &str, args: &Value) -> io::Result<()> {
    let cols = terminal::size().map(|(cols, _)| cols).unwrap_or(80);
    let args_preview = tool_call_args_preview(name, args, cols);
    queue!(
        out,
        Print(NL),
        Print(theme::GUTTER),
        SetForegroundColor(theme::TOOL),
        Print("* "),
        SetAttribute(Attribute::Bold),
        Print(name),
        SetAttribute(Attribute::NormalIntensity),
        ResetColor,
        SetForegroundColor(theme::DIM),
        Print(format!("({args_preview})")),
        Clear(ClearType::UntilNewLine),
        ResetColor,
        Print(NL),
    )
}

fn tool_call_args_preview(name: &str, args: &Value, cols: u16) -> String {
    let fixed_cols = theme::GUTTER.chars().count() + 2 + name.chars().count() + 2;
    let preview_cols = usize::from(cols.saturating_sub(1)).saturating_sub(fixed_cols);
    preview_fit(&args.to_string(), preview_cols)
}

pub(crate) fn print_tool_result<W: Write>(
    out: &mut W,
    content: &str,
    is_error: bool,
) -> io::Result<()> {
    let body_color = if is_error { theme::ERROR } else { theme::DIM };
    let body_lines: Vec<&str> = content.lines().map(|line| line.trim_end()).collect();
    let shown = body_lines.len().min(COLLAPSED_TOOL_RESULT_LINES);

    for (i, line) in body_lines.iter().take(shown).enumerate() {
        let arrow = if i == 0 { "  -> " } else { "     " };
        queue!(
            out,
            Print(theme::GUTTER),
            SetForegroundColor(theme::DIM),
            Print(arrow),
            SetForegroundColor(body_color),
            Print(*line),
            ResetColor,
            Print(NL),
        )?;
    }

    if body_lines.len() > shown {
        let more = body_lines.len() - shown;
        queue!(
            out,
            Print(theme::GUTTER),
            SetForegroundColor(theme::DIM),
            Print(format!(
                "     +{} more line{}",
                more,
                if more == 1 { "" } else { "s" }
            )),
            ResetColor,
            Print(NL),
        )?;
    }
    Ok(())
}

pub(crate) fn print_error<W: Write>(out: &mut W, text: &str) -> io::Result<()> {
    queue!(
        out,
        Print(NL),
        Print(theme::GUTTER),
        SetForegroundColor(theme::ERROR),
        SetAttribute(Attribute::Bold),
        Print("x "),
        SetAttribute(Attribute::NormalIntensity),
        Print(text),
        ResetColor,
        Print(NL),
    )
}

pub(crate) fn print_separator<W: Write>(out: &mut W) -> io::Result<()> {
    queue!(
        out,
        Print(NL),
        SetForegroundColor(theme::DIM),
        Print("──"),
        ResetColor,
        Print(NL),
    )
}

pub(crate) fn print_intro<W: Write>(out: &mut W, model: &str, directory: &str) -> io::Result<()> {
    queue!(
        out,
        SetForegroundColor(theme::USER),
        SetAttribute(Attribute::Bold),
        Print(">_ "),
        SetAttribute(Attribute::NormalIntensity),
        ResetColor,
        SetAttribute(Attribute::Bold),
        Print(format!("Tiny (v{})", env!("CARGO_PKG_VERSION"))),
        SetAttribute(Attribute::NormalIntensity),
        Print(NL),
        SetForegroundColor(theme::DIM),
        Print(format!("model:     {}", model)),
        Print(NL),
        Print(format!("directory: {}", directory)),
        Print(NL),
        Print("Type a message and press Enter. Type / for commands."),
        ResetColor,
        Print(NL),
    )
}

pub(crate) fn preview(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{short}...")
    } else {
        short
    }
}

fn preview_fit(text: &str, max_cols: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_cols).collect();
    if chars.next().is_none() {
        return short;
    }
    if max_cols <= 3 {
        ".".repeat(max_cols)
    } else {
        let prefix: String = text.chars().take(max_cols - 3).collect();
        format!("{prefix}...")
    }
}

fn heading_body(line: &str) -> Option<&str> {
    let mut hashes = 0;
    let bytes = line.as_bytes();
    while hashes < bytes.len() && bytes[hashes] == b'#' {
        hashes += 1;
    }
    if hashes == 0 || hashes > 3 {
        return None;
    }
    if bytes.get(hashes) == Some(&b' ') {
        Some(&line[hashes + 1..])
    } else {
        None
    }
}

fn bullet_body(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'-' || bytes[0] == b'*') && bytes[1] == b' ' {
        Some(&line[2..])
    } else {
        None
    }
}

fn print_inline_code<W: Write>(out: &mut W, text: &str) -> io::Result<()> {
    let mut in_code = false;
    let mut buf = String::new();

    for c in text.chars() {
        if c == '`' {
            if !buf.is_empty() {
                if in_code {
                    queue!(
                        out,
                        SetForegroundColor(theme::TOOL),
                        Print(std::mem::take(&mut buf)),
                        ResetColor,
                    )?;
                } else {
                    queue!(out, Print(std::mem::take(&mut buf)))?;
                }
            }
            in_code = !in_code;
            continue;
        }
        buf.push(c);
    }

    if !buf.is_empty() {
        if in_code {
            // Unbalanced trailing backtick: emit as literal.
            queue!(out, Print("`"), Print(buf))?;
        } else {
            queue!(out, Print(buf))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::tool_call_args_preview;
    use crate::tui::theme;
    use serde_json::json;

    #[test]
    fn tool_call_preview_fits_terminal_width() {
        let cols = 40;
        let name = "bash";
        let args = json!({
            "command": "git add -N src/tui.rs src/tui/commands.rs src/tui/prompt.rs"
        });

        let preview = tool_call_args_preview(name, &args, cols);
        let rendered_cols =
            theme::GUTTER.chars().count() + 2 + name.chars().count() + 2 + preview.chars().count();

        assert!(rendered_cols <= usize::from(cols - 1));
    }
}
