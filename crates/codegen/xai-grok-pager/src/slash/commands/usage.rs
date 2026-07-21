//! `/usage` -- show local token-usage info (BYOK fork has no billing page).

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};

/// Show local token-usage summary.
///
/// `/usage`      -- show current session token/context usage
/// `/usage show` -- same as above
///
/// The original `manage` subcommand (opens x.ai billing) is removed in the
/// Free/BYOK fork because there is no xAI subscription or credits to manage.
pub struct UsageCommand;

impl SlashCommand for UsageCommand {
    fn name(&self) -> &str {
        "usage"
    }

    /// `/cost` is the minimal-mode alias for the same local usage summary.
    fn aliases(&self) -> &[&str] {
        &["cost"]
    }

    fn description(&self) -> &str {
        "View local token / context usage"
    }

    fn usage(&self) -> &str {
        "/usage [show]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("show")
    }

    fn suggest_args(&self, _ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        Some(vec![ArgItem {
            display: "show".to_string(),
            match_text: "show".to_string(),
            insert_text: "show".to_string(),
            description: "View local token / context usage".to_string(),
        }])
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let arg = args.trim();
        match arg {
            "" | "show" => CommandResult::Action(Action::ShowUsage),
            "manage" => CommandResult::Error(
                "Billing management is not available in BYOK mode.".to_string(),
            ),
            _ => CommandResult::Error(format!(
                "Unknown argument: {arg}. Use /usage show"
            )),
        }
    }
}
