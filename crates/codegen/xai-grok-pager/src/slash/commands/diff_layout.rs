//! `/diff-layout` -- toggle or set unified vs side-by-side edit diffs.

use crate::app::actions::Action;
use crate::scrollback::blocks::DiffLayout;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

pub struct DiffLayoutCommand;

impl SlashCommand for DiffLayoutCommand {
    fn name(&self) -> &str {
        "diff-layout"
    }

    fn aliases(&self) -> &[&str] {
        &["difflayout"]
    }

    fn description(&self) -> &str {
        "Toggle edit diffs between unified and side-by-side"
    }

    fn usage(&self) -> &str {
        "/diff-layout [unified|side_by_side]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("unified | side_by_side")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Action(Action::ToggleDiffLayout);
        }
        match DiffLayout::parse(trimmed) {
            Some(layout) => {
                CommandResult::Action(Action::SetDiffLayout(layout.as_str().to_string()))
            }
            None => CommandResult::Error(format!(
                "Unknown layout: {trimmed}. Use unified or side_by_side"
            )),
        }
    }
}
