use crossterm::style::Color;

pub(crate) const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(crate) const USER: Color = Color::Cyan;
pub(crate) const USER_BG: Color = Color::AnsiValue(236);
pub(crate) const ASSISTANT: Color = Color::Green;
pub(crate) const TOOL: Color = Color::Yellow;
pub(crate) const DIM: Color = Color::DarkGrey;
pub(crate) const ERROR: Color = Color::Red;
pub(crate) const GUTTER: &str = " ";
