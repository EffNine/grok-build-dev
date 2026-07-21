//! `/login` -- alias for BYOK setup in the free/BYOK fork (OAuth removed).

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::commands::byok::parse_byok_args;

pub struct LoginCommand;

impl SlashCommand for LoginCommand {
    fn name(&self) -> &str {
        "login"
    }

    fn description(&self) -> &str {
        "Configure your API key and model endpoint (same as /byok)"
    }

    fn usage(&self) -> &str {
        "/login <api_key> <base_url>"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        parse_byok_args(args)
    }
}
