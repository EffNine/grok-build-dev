//! `x.ai/auth/*` and legacy `x.ai/{get,set}ApiKey` extension handlers.
//!
//! These methods let the client read/write the API key via the agent and
//! drive the OAuth login flow. The agent is the single source of truth for
//! `auth.json`.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::ExtMethodResult;

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "x.ai/auth/getBearerToken" => handle_get_bearer_token(agent).await,
        "x.ai/getApiKey" => handle_get_api_key(),
        "x.ai/setApiKey" => handle_set_api_key(args),
        "x.ai/byok/configure" => handle_byok_configure(agent, args).await,
        "x.ai/auth/submit_code" => handle_submit_code(agent, args),
        "x.ai/auth/get_url" => handle_get_url(agent).await,
        "x.ai/auth/cancel" => handle_cancel(agent, args),
        "x.ai/auth/logout" => handle_logout(agent, args).await,
        "x.ai/auth/info" => handle_info(agent),
        "x.ai/auth/check_subscription" => handle_check_subscription(agent).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

/// Stop an in-flight interactive login (device poll / loopback wait).
/// Idempotent: no-op when nothing is waiting.
///
/// When `request_seq` is present, only that attempt is cancelled — a delayed
/// cancel cannot tear down a successor login that already replaced it.
fn handle_cancel(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct CancelParams {
        #[serde(default)]
        request_seq: Option<u64>,
    }
    let params: CancelParams =
        serde_json::from_str(args.params.get()).unwrap_or(CancelParams { request_seq: None });
    match params.request_seq {
        Some(seq) => agent.interactive_auth.cancel_for_client_seq(seq),
        None => agent.interactive_auth.cancel(),
    }
    to_raw_response(&serde_json::json!({ "cancelled": true }))
}

async fn handle_get_bearer_token(agent: &MvpAgent) -> ExtResult {
    let token = match agent.auth_manager.get_valid_token().await {
        Ok(token) => Some(token),
        Err(_) => agent
            .sampling_config
            .borrow()
            .api_key
            .clone()
            .or_else(|| agent.auth_manager.current().map(|a| a.key)),
    };
    ExtMethodResult::success(serde_json::json!({ "token": token }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

fn handle_get_api_key() -> ExtResult {
    let key = crate::agent::auth_method::read_xai_api_key_env().ok();
    ExtMethodResult::success(serde_json::json!({ "key": key }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

fn handle_set_api_key(args: &acp::ExtRequest) -> ExtResult {
    let params: serde_json::Value = parse_params(args)?;
    let key = params.get("key").and_then(|v| v.as_str());
    let grok_home = crate::util::grok_home::grok_home();
    if let Some(k) = key {
        if k.is_empty() {
            crate::auth::clear_api_key(&grok_home)
                .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;
            // SAFETY: ext_method is single-threaded per agent
            unsafe { std::env::remove_var("XAI_API_KEY") };
        } else {
            crate::auth::store_api_key(&grok_home, k)
                .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;
            // SAFETY: ext_method is single-threaded per agent
            unsafe { std::env::set_var("XAI_API_KEY", k) };
        }
    } else {
        crate::auth::clear_api_key(&grok_home)
            .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;
        // SAFETY: ext_method is single-threaded per agent
        unsafe { std::env::remove_var("XAI_API_KEY") };
    }
    ExtMethodResult::success(serde_json::json!({ "ok": true }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

/// Persist a global API key + provider base URL, then fetch the model catalog.
///
/// Writes both `models_base_url` and `xai_api_base_url` so chat, `/models`
/// fetch, and xAI-direct tools (image/video) all use the same OpenAI-compatible
/// endpoint after restart.
async fn handle_byok_configure(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct ByokParams {
        key: String,
        base_url: String,
        #[serde(default)]
        models_list_url: Option<String>,
    }
    let params: ByokParams = parse_params(args)?;
    let key = params.key.trim();
    let base_url = params.base_url.trim().trim_end_matches('/');
    if key.is_empty() || base_url.is_empty() {
        return Err(acp::Error::invalid_params()
            .data("key and base_url are required (e.g. /byok <api_key> <base_url>)"));
    }

    let grok_home = crate::util::grok_home::grok_home();
    crate::auth::store_api_key(&grok_home, key)
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;
    // SAFETY: ext_method is single-threaded per agent
    unsafe {
        std::env::set_var("XAI_API_KEY", key);
        std::env::set_var("GROK_MODELS_BASE_URL", base_url);
        std::env::set_var("GROK_XAI_API_BASE_URL", base_url);
        if let Some(ref list_url) = params.models_list_url {
            let list_url = list_url.trim();
            if !list_url.is_empty() {
                std::env::set_var("GROK_MODELS_LIST_URL", list_url);
            }
        }
    }

    let config_path = grok_home.join("config.toml");
    persist_byok_endpoints_to(&config_path, base_url, params.models_list_url.as_deref())
        .map_err(|e| acp::Error::internal_error().data(e))?;

    {
        let mut sampling = agent.sampling_config.borrow_mut();
        sampling.api_key = Some(key.to_string());
    }
    {
        let mut cfg = agent.cfg.borrow_mut();
        cfg.endpoints.models_base_url = Some(base_url.to_string());
        cfg.endpoints.xai_api_base_url = base_url.to_string();
        if let Some(ref list_url) = params.models_list_url {
            let list_url = list_url.trim();
            if !list_url.is_empty() {
                cfg.endpoints.models_list_url = Some(list_url.to_string());
            }
        } else {
            // Stale list URL from a previous provider must not stick.
            cfg.endpoints.models_list_url = None;
        }
    }

    agent.set_auth_method(acp::AuthMethodId::new(
        crate::agent::auth_method::XAI_API_KEY_METHOD_ID,
    ));
    agent.sync_process_static_api_key(None);

    let cfg_snapshot = agent.cfg.borrow().clone();
    agent.models_manager.apply_config(cfg_snapshot);
    agent.models_manager.on_auth_changed().await;

    let model_count = agent.models_manager.models().len();
    ExtMethodResult::success(serde_json::json!({
        "ok": true,
        "base_url": base_url,
        "model_count": model_count,
    }))
    .to_ext_response()
    .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

/// Write BYOK endpoint URLs into `config.toml`.
///
/// Persists `models_base_url` and `xai_api_base_url` to the same value so a
/// restart reloads the OpenAI-compatible (or xAI) URL without requiring env
/// vars. Clears `models_list_url` when the caller omits it.
fn persist_byok_endpoints_to(
    config_path: &std::path::Path,
    base_url: &str,
    models_list_url: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = crate::util::config::read_to_string_or_empty(config_path)
        .map_err(|e| format!("read config.toml: {e}"))?;
    let mut doc = if existing.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        existing
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("parse config.toml: {e}"))?
    };
    if !doc.contains_key("endpoints") {
        doc["endpoints"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let endpoints = doc["endpoints"]
        .as_table_mut()
        .ok_or_else(|| "[endpoints] is not a table".to_string())?;
    endpoints["models_base_url"] = toml_edit::value(base_url);
    endpoints["xai_api_base_url"] = toml_edit::value(base_url);
    if let Some(list_url) = models_list_url.map(str::trim).filter(|s| !s.is_empty()) {
        endpoints["models_list_url"] = toml_edit::value(list_url);
    } else {
        endpoints.remove("models_list_url");
    }
    crate::util::config::atomic_write_string(config_path, &doc.to_string())
        .map_err(|e| format!("write config.toml: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod byok_persist_tests {
    use super::persist_byok_endpoints_to;
    use crate::agent::config::{Config, EndpointsConfig};

    #[test]
    fn persist_byok_endpoints_saves_openai_url_to_both_fields() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        persist_byok_endpoints_to(&config_path, "https://api.openai.com/v1", None)
            .expect("persist");

        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            raw.contains("models_base_url = \"https://api.openai.com/v1\""),
            "models_base_url missing in:\n{raw}"
        );
        assert!(
            raw.contains("xai_api_base_url = \"https://api.openai.com/v1\""),
            "xai_api_base_url missing in:\n{raw}"
        );
        assert!(
            !raw.contains("models_list_url"),
            "stale models_list_url should be cleared:\n{raw}"
        );

        let value: toml::Value = toml::from_str(&raw).unwrap();
        let cfg = Config::new_from_toml_cfg(&value).expect("reload");
        assert_eq!(
            cfg.endpoints.models_base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(cfg.endpoints.xai_api_base_url, "https://api.openai.com/v1");
        assert_eq!(
            cfg.endpoints.resolve_inference_base_url(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn persist_byok_endpoints_saves_xai_url_and_clears_stale_list() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[endpoints]
models_list_url = "https://old.example/v1/models"
"#,
        )
        .unwrap();

        persist_byok_endpoints_to(&config_path, "https://api.x.ai/v1", None).expect("persist");
        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("models_base_url = \"https://api.x.ai/v1\""));
        assert!(raw.contains("xai_api_base_url = \"https://api.x.ai/v1\""));
        assert!(
            !raw.contains("models_list_url"),
            "expected models_list_url cleared:\n{raw}"
        );
    }

    #[test]
    fn inference_falls_back_to_xai_api_not_proxy_when_models_base_unset() {
        let cfg = EndpointsConfig {
            models_base_url: None,
            xai_api_base_url: "https://api.x.ai/v1".to_string(),
            cli_chat_proxy_base_url: None,
            ..Default::default()
        };
        assert_eq!(
            cfg.resolve_inference_base_url(),
            "https://api.x.ai/v1",
            "API-key-only BYOK must not route chat to cli-chat-proxy"
        );
        assert_ne!(
            cfg.resolve_inference_base_url(),
            cfg.proxy_url(),
            "proxy must not be the inference fallback in this fork"
        );
    }
}

/// Handle auth code submission from TUI.
fn handle_submit_code(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct SubmitCodeParams {
        code: String,
    }

    let params: SubmitCodeParams = serde_json::from_str(args.params.get())
        .map_err(|e| acp::Error::invalid_params().data(format!("invalid params: {e}")))?;

    match agent.interactive_auth.submit_code(params.code) {
        Ok(()) => to_raw_response(&serde_json::json!({ "submitted": true })),
        Err(crate::auth::single_flight::SubmitCodeError::SendFailed(e)) => {
            Err(acp::Error::internal_error().data(format!("failed to submit auth code: {e}")))
        }
        Err(crate::auth::single_flight::SubmitCodeError::NoPendingAttempt) => {
            Err(acp::Error::invalid_params().data("no pending auth session"))
        }
    }
}

/// Awaits the auth URL from the oneshot channel (blocks until ready).
async fn handle_get_url(agent: &MvpAgent) -> ExtResult {
    let rx = agent.interactive_auth.take_url_rx();
    // `None` when no URL was sent (cached creds, early error, second poll):
    // report mode as `null` rather than mislabeling it `loopback`.
    let (auth_url, mode) = match rx {
        Some(rx) => match rx.await {
            Ok(info) => (Some(info.url), Some(info.mode)),
            Err(_) => (None, None),
        },
        None => (None, None),
    };
    to_raw_response(&serde_json::json!({
        "auth_url": auth_url,
        // `external_provider` kept for older clients; `mode` is authoritative.
        "external_provider": mode.is_some_and(|m| m.is_external_provider()),
        "mode": mode.map(|m| m.as_wire_str()),
    }))
}

async fn handle_logout(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct LogoutParams {
        scope: Option<String>,
    }

    let params: LogoutParams = serde_json::from_str(args.params.get())
        .map_err(|e| acp::Error::invalid_params().data(format!("invalid params: {e}")))?;

    // Stop any in-flight login so it cannot write credentials back after logout.
    agent.interactive_auth.cancel();

    let result = crate::auth::perform_logout(&agent.auth_manager, params.scope.as_deref())
        .map_err(|e| acp::Error::internal_error().data(format!("failed to logout: {e}")))?;
    // Free/BYOK fork: always clear the stored API key on logout.
    let grok_home = crate::util::grok_home::grok_home();
    let _ = crate::auth::clear_api_key(&grok_home);
    unsafe { std::env::remove_var("XAI_API_KEY") };
    agent.sampling_config.borrow_mut().api_key = None;

    // `auth.lifecycle` (not `auth`) avoids colliding with the pre-existing
    // per-request `AuthManager::auth()` `#[instrument]` span.
    tracing::info_span!("auth.lifecycle", action = "logout", success = true).in_scope(|| {});

    agent.models_manager.on_auth_changed().await;

    to_raw_response(&serde_json::json!({
        "ok": true,
        "was_logged_in": result.was_logged_in,
        "email": result.email,
        "api_key_still_set": false,
    }))
}

/// Single-shot subscription re-check (retry button on paywall screen).
///
/// Calls `retry_subscription_check()`, then returns the updated auth
/// response with gate info so the pager can refresh the gate state.
async fn handle_check_subscription(agent: &MvpAgent) -> ExtResult {
    agent.retry_subscription_check().await;
    let response = agent.auth_response_with_meta();
    to_raw_response(&serde_json::json!({
        "authenticated": response.meta.is_some(),
        "meta": response.meta,
    }))
}

/// Returns current auth method ID, user profile fields, and team/principal
/// metadata.
fn handle_info(agent: &MvpAgent) -> ExtResult {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AuthInfoResponse {
        method_id: Option<String>,
        email: Option<String>,
        first_name: Option<String>,
        last_name: Option<String>,
        /// `grok-asset://` URL resolved by the Electron protocol handler,
        /// or a full `http(s)://` URL passed through unchanged.
        profile_image_url: Option<String>,
        team_id: Option<String>,
        team_name: Option<String>,
        team_role: Option<String>,
        organization_id: Option<String>,
        organization_name: Option<String>,
        organization_role: Option<String>,
        principal_type: Option<String>,
        principal_id: Option<String>,
        user_blocked_reason: Option<String>,
        team_blocked_reasons: Vec<String>,
        coding_data_retention_opt_out: bool,
    }

    let method_id = agent
        .auth_method_id
        .load()
        .as_ref()
        .map(|m| m.0.to_string());
    let auth = agent.auth_manager.current();
    let raw_asset_id = auth.as_ref().and_then(|a| a.profile_image_asset_id.clone());

    // Return a grok-asset:// URL that the Electron renderer resolves at
    // display time via a custom protocol handler. The handler proxies
    // through cli-chat-proxy's /asset endpoint; Electron's HTTP cache
    // handles reuse. No disk-cache or network call needed here.
    let profile_image_url = match raw_asset_id.as_deref().filter(|k| !k.is_empty()) {
        Some(key) if key.starts_with("http://") || key.starts_with("https://") => {
            Some(key.to_owned())
        }
        Some(key) => Some(format!("grok-asset:///{key}")),
        None => None,
    };
    to_raw_response(&AuthInfoResponse {
        method_id,
        email: auth.as_ref().and_then(|a| a.email.clone()),
        first_name: auth.as_ref().and_then(|a| a.first_name.clone()),
        last_name: auth.as_ref().and_then(|a| a.last_name.clone()),
        profile_image_url,
        team_id: auth.as_ref().and_then(|a| a.team_id.clone()),
        team_name: auth.as_ref().and_then(|a| a.team_name.clone()),
        team_role: auth.as_ref().and_then(|a| a.team_role.clone()),
        organization_id: auth.as_ref().and_then(|a| a.organization_id.clone()),
        organization_name: auth.as_ref().and_then(|a| a.organization_name.clone()),
        organization_role: auth.as_ref().and_then(|a| a.organization_role.clone()),
        principal_type: auth.as_ref().and_then(|a| a.principal_type.clone()),
        principal_id: auth.as_ref().and_then(|a| a.principal_id.clone()),
        user_blocked_reason: auth.as_ref().and_then(|a| a.user_blocked_reason.clone()),
        team_blocked_reasons: auth
            .as_ref()
            .map(|a| a.team_blocked_reasons.clone())
            .unwrap_or_default(),
        // No credential ⇒ unknown privacy state: report opted-out (fail closed),
        // matching `AuthManager::allows_data_collection` / GrokAuth Default.
        coding_data_retention_opt_out: auth
            .as_ref()
            .map(|a| a.coding_data_retention_opt_out)
            .unwrap_or_else(crate::auth::default_coding_data_retention_opt_out),
    })
}
