//! `/budget` -- set or view the session output-token budget.
//!
//! Queues to the shell (`BuiltinAction::Budget`) so consumption tracking and
//! nudges stay session-authoritative.

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Set or view the cumulative output-token budget for this session.
///
/// `/budget`           -- show current status
/// `/budget 500k`      -- set budget to 500,000 tokens
/// `/budget clear`     -- clear the budget
pub struct BudgetCommand;

impl SlashCommand for BudgetCommand {
    fn name(&self) -> &str {
        "budget"
    }

    fn description(&self) -> &str {
        "Set or view the session output-token budget"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/budget [amount|clear|status]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("amount (e.g. 500k) | clear | status")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let text = if args.trim().is_empty() {
            "/budget".to_string()
        } else {
            format!("/budget {}", args.trim())
        };
        CommandResult::QueueCommand(text)
    }
}
