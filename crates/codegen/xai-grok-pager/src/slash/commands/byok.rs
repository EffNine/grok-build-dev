//! `/byok` -- configure a global API key + models base URL and fetch all models.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

pub struct ByokCommand;

impl SlashCommand for ByokCommand {
    fn name(&self) -> &str {
        "byok"
    }

    fn description(&self) -> &str {
        "Configure your API key and model endpoint (fetches all available models)"
    }

    fn usage(&self) -> &str {
        "/byok <api_key> <base_url>"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        parse_byok_args(args)
    }
}

pub(crate) fn parse_byok_args(args: &str) -> CommandResult {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return CommandResult::Message(
            "Usage: /byok <api_key> <base_url>\n\
             Example: /byok sk-... https://api.openai.com/v1\n\
             Or set XAI_API_KEY and GROK_MODELS_BASE_URL, then restart."
                .to_string(),
        );
    }
    let mut parts = trimmed.split_whitespace();
    let key = parts.next().unwrap_or("").to_string();
    let base_url = parts.next().unwrap_or("").to_string();
    let list_url = parts.next().map(|s| s.to_string());
    if key.is_empty() || base_url.is_empty() {
        return CommandResult::Message(
            "Usage: /byok <api_key> <base_url> [models_list_url]".to_string(),
        );
    }
    CommandResult::Action(Action::ConfigureByok {
        key,
        base_url,
        models_list_url: list_url,
    })
}
