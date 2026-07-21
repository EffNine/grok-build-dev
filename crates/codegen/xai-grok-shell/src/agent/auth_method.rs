use agent_client_protocol as acp;

use crate::agent::config::ModelEntry;
use crate::auth::PreferredAuthMethod;

/// Shared, live handle to the agent's current ACP auth method id.
///
/// `Arc` so a clone can cross the per-session-thread boundary at spawn; the
/// `ArcSwapOption` interior lets the agent's `authenticate` handler publish a
/// new method that every running session's per-turn auth gate observes on its
/// next turn -- no re-spawn. `None` until the first `authenticate`. Auth is
/// process-global (one user, one `AuthManager`), so all sessions sharing one
/// cell is correct.
pub(crate) type SharedAuthMethodId = std::sync::Arc<arc_swap::ArcSwapOption<acp::AuthMethodId>>;

/// Construct a [`SharedAuthMethodId`]. `None` is the pre-`authenticate` state.
pub(crate) fn new_shared_auth_method_id(initial: Option<acp::AuthMethodId>) -> SharedAuthMethodId {
    std::sync::Arc::new(arc_swap::ArcSwapOption::new(
        initial.map(std::sync::Arc::new),
    ))
}

/// Env var that, when set, advertises `xai.api_key` as a viable auth method.
///
/// Kept as a constant so test code and the production check stay in sync.
pub const XAI_API_KEY_ENV_VAR: &str = "XAI_API_KEY";

/// Legacy env var name. Checked as a fallback when `XAI_API_KEY` is not set,
/// so existing deployments that use the old name keep working.
pub const LEGACY_XAI_API_KEY_ENV_VAR: &str = "GROK_CODE_XAI_API_KEY";

/// Read the API key from the environment.
///
/// Checks `XAI_API_KEY` first, then falls back to the legacy
/// `GROK_CODE_XAI_API_KEY` for backward compatibility.
pub fn read_xai_api_key_env() -> Result<String, std::env::VarError> {
    std::env::var(XAI_API_KEY_ENV_VAR).or_else(|_| std::env::var(LEGACY_XAI_API_KEY_ENV_VAR))
}

/// Returns `true` if either `XAI_API_KEY` or `GROK_CODE_XAI_API_KEY` is set.
pub fn has_xai_api_key_env() -> bool {
    read_xai_api_key_env().is_ok()
}

/// Whether `xai.api_key` should be advertised (and pushed FIRST) when building
/// the `auth_methods` list at `initialize()` time.
///
/// Regression: `xai.api_key` must stay first when only per-model credentials
/// exist (no global `XAI_API_KEY`). Deferring it made BYOK users hit the login
/// screen because the pager uses `auth_methods.first()` for startup metadata.
///
/// [`build_auth_methods`] consumes this predicate and pins the ordering;
/// its tests catch call-site and predicate regressions.
///
/// Probes `std::env` at call time and consults each `ModelEntry` for a
/// resolvable api_key/env_key -- both inputs can change between calls, so the
/// result is not cached.
///
/// `disable_api_key_auth` (`[grok_com_config] disable_api_key_auth` /
/// `GROK_DISABLE_API_KEY_AUTH`) is the admin kill switch: when true the
/// method is never advertised, regardless of available credentials, so
/// `XAI_API_KEY` can't bypass a deployment's forced IdP login.
pub fn should_advertise_xai_api_key<'a, I>(_disable_api_key_auth: bool, _models: I) -> bool
where
    I: IntoIterator<Item = &'a ModelEntry>,
{
    // Free/BYOK fork: always advertise API-key auth. OAuth / IdP kill-switches
    // are ignored so the product path never falls through to grok.com login.
    true
}

/// Inputs to [`build_auth_methods`].
///
/// Booleans are computed by the caller (`MvpAgent::initialize()`) because they
/// depend on async side effects (token refresh) and shared mutable state
/// (`AuthManager`). The list-construction logic itself is pure so it can be
/// unit-tested without any of that machinery.
pub struct AuthMethodsBuildInputs<'a> {
    /// True if `xai.api_key` should be advertised AT ALL. Caller computes via
    /// [`should_advertise_xai_api_key`]. When `preferred_method` is `Oidc`,
    /// this is ignored (API key is never advertised under that pin).
    pub has_external_api_key: bool,
    /// True if a cached session token is available (either present at startup
    /// or recovered via silent refresh).
    pub has_cached_token: bool,
    /// True if enterprise OIDC is configured. Mutually exclusive with the
    /// default `grok.com` method.
    pub has_enterprise_oidc: bool,
    /// Required when `has_enterprise_oidc` is true; ignored otherwise.
    pub enterprise_oidc_issuer: Option<&'a str>,
    /// Optional display label for the login method (`grok.com` or `oidc`).
    pub login_label: Option<&'a str>,
    /// True if `grok_com_config.auth_provider_command` is configured (sets
    /// `meta.external_provider = true` on the `grok.com` method).
    pub has_auth_provider_command: bool,
    /// Config pin (`[auth] preferred_method`). `None` keeps multi-method
    /// fallthrough; `Some` is fail-closed (only that method family).
    pub preferred_method: Option<PreferredAuthMethod>,
}

/// Output of [`build_auth_methods`].
pub struct BuiltAuthMethods {
    /// Auth methods in advertised order. ORDER IS THE CONTRACT: the pager's
    /// `startup_auth_metadata()` reads `methods.first()` to decide whether
    /// interactive login is needed.
    pub methods: Vec<acp::AuthMethod>,
    /// The default `auth_method_id` to install on the agent. When unpinned,
    /// `cached_token` wins over `xai.api_key` when both are present. When
    /// pinned, only the preferred method may appear; `None` means unavailable
    /// (fail auth — no cross-method fallthrough).
    pub default_auth_method_id: Option<acp::AuthMethodId>,
}

/// Build the `auth_methods` list and default `auth_method_id` from
/// pre-computed inputs.
///
/// Free/BYOK fork: always advertise only `xai.api_key`. OAuth / cached-token /
/// enterprise OIDC methods are never offered on the product path. Inputs are
/// accepted for call-site compatibility but ignored.
pub fn build_auth_methods(_inputs: AuthMethodsBuildInputs<'_>) -> BuiltAuthMethods {
    BuiltAuthMethods {
        methods: vec![xai_api_key_auth_method()],
        default_auth_method_id: Some(acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID)),
    }
}

#[allow(dead_code)] // retained for reference; free/BYOK fork never pins OIDC
fn build_pinned_api_key(has_external_api_key: bool) -> BuiltAuthMethods {
    if !has_external_api_key {
        xai_grok_telemetry::unified_log::warn(
            "auth: preferred_method=api_key but no API key credentials available",
            None,
            None,
        );
        return BuiltAuthMethods {
            methods: Vec::new(),
            default_auth_method_id: None,
        };
    }
    BuiltAuthMethods {
        methods: vec![xai_api_key_auth_method()],
        default_auth_method_id: Some(acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID)),
    }
}

#[allow(dead_code)] // retained for reference; free/BYOK fork never advertises OIDC
fn build_pinned_oidc(
    has_cached_token: bool,
    has_enterprise_oidc: bool,
    enterprise_oidc_issuer: Option<&str>,
    login_label: Option<&str>,
    has_auth_provider_command: bool,
) -> BuiltAuthMethods {
    let mut methods: Vec<acp::AuthMethod> = Vec::new();
    let mut default_auth_method_id: Option<acp::AuthMethodId> = None;

    if has_cached_token {
        methods.push(cached_token_auth_method());
        default_auth_method_id = Some(acp::AuthMethodId::new(CACHED_TOKEN_AUTH_METHOD_ID));
    }

    push_interactive_login(
        &mut methods,
        has_enterprise_oidc,
        enterprise_oidc_issuer,
        login_label,
        has_auth_provider_command,
    );

    BuiltAuthMethods {
        methods,
        default_auth_method_id,
    }
}

#[allow(dead_code)] // retained for reference; free/BYOK fork uses build_auth_methods only
fn build_unpinned(
    has_external_api_key: bool,
    has_cached_token: bool,
    has_enterprise_oidc: bool,
    enterprise_oidc_issuer: Option<&str>,
    login_label: Option<&str>,
    has_auth_provider_command: bool,
) -> BuiltAuthMethods {
    let mut methods: Vec<acp::AuthMethod> = Vec::new();
    let mut default_auth_method_id: Option<acp::AuthMethodId> = None;

    if has_external_api_key {
        methods.push(xai_api_key_auth_method());
        default_auth_method_id = Some(acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID));
    }

    if has_cached_token {
        methods.push(cached_token_auth_method());
        // cached_token wins over xai.api_key for default_auth_method_id so
        // is_session_based_auth() returns true and OIDC refresh stays alive.
        let overrode_api_key = default_auth_method_id.is_some();
        default_auth_method_id = Some(acp::AuthMethodId::new(CACHED_TOKEN_AUTH_METHOD_ID));
        if overrode_api_key {
            xai_grok_telemetry::unified_log::info(
                "auth method priority: cached_token overrides xai.api_key for default_auth_method_id",
                None,
                Some(serde_json::json!({
                    "has_external_api_key": has_external_api_key,
                    "has_cached_token": has_cached_token,
                })),
            );
        }
    }

    push_interactive_login(
        &mut methods,
        has_enterprise_oidc,
        enterprise_oidc_issuer,
        login_label,
        has_auth_provider_command,
    );

    BuiltAuthMethods {
        methods,
        default_auth_method_id,
    }
}

#[allow(dead_code)] // retained for reference; free/BYOK fork never pushes interactive login
fn push_interactive_login(
    methods: &mut Vec<acp::AuthMethod>,
    has_enterprise_oidc: bool,
    enterprise_oidc_issuer: Option<&str>,
    login_label: Option<&str>,
    has_auth_provider_command: bool,
) {
    if has_enterprise_oidc {
        // Caller invariant: `enterprise_oidc_issuer` MUST be `Some(...)` when
        // `has_enterprise_oidc` is true. Production callers derive both from
        // the same `cfg.grok_com_config.oidc` Option, so the inconsistent
        // `(true, None)` combination is a programmer error -- panic loudly
        // (matches the original `cfg.grok_com_config.oidc.as_ref().unwrap()`
        // call in `MvpAgent::initialize()` before this refactor).
        let issuer = enterprise_oidc_issuer
            .expect("enterprise_oidc_issuer is required when has_enterprise_oidc is true");
        methods.push(oidc_auth_method(issuer, login_label));
    } else {
        methods.push(grok_com_auth_method(login_label, has_auth_provider_command));
    }
}

/// ACP session auth method. Use `is_session_based_method` for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethodKind {
    XaiApiKey,
    CachedToken,
    GrokCom,
    Oidc,
    Unknown,
}

impl AuthMethodKind {
    pub fn from_id(id: &acp::AuthMethodId) -> Self {
        match id.0.as_ref() {
            XAI_API_KEY_METHOD_ID => Self::XaiApiKey,
            CACHED_TOKEN_AUTH_METHOD_ID => Self::CachedToken,
            GROK_COM_METHOD_ID => Self::GrokCom,
            OIDC_METHOD_ID => Self::Oidc,
            _ => Self::Unknown,
        }
    }

    /// API key auth: no auth.json, no refresh, no user interaction.
    pub fn is_api_key(self) -> bool {
        matches!(self, Self::XaiApiKey)
    }

    /// `true` for session-based methods (cached_token, grok.com, oidc).
    pub fn is_session_based(self) -> bool {
        matches!(self, Self::CachedToken | Self::GrokCom | Self::Oidc)
    }

    /// Requires user interaction (browser, OIDC redirect, or external auth command).
    pub fn needs_interactive_login(self) -> bool {
        matches!(self, Self::GrokCom | Self::Oidc)
    }

    pub fn auth_error_message(self) -> &'static str {
        if self.is_session_based() {
            AUTH_ERROR_SESSION_EXPIRED
        } else {
            AUTH_ERROR_API_KEY
        }
    }
}

/// `true` for session-based ACP methods (cached_token, grok.com, oidc).
pub fn is_session_based_method(method_id: &acp::AuthMethodId) -> bool {
    AuthMethodKind::from_id(method_id).is_session_based()
}

/// Per-model BYOK status: whether the selected model carries its own
/// `[model.*]` `api_key`/`env_key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelByok {
    /// Model has its own per-model key (not refreshable).
    Byok,
    /// Model has no per-model key (session auth governs).
    NotByok,
    /// Config couldn't be loaded/parsed — BYOK status indeterminate.
    Unknown,
}

impl ModelByok {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Byok => "byok",
            Self::NotByok => "not_byok",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether this session+model uses a refreshable session token.
///
/// Gates on stable inputs, not `Credentials.auth_type`: that field collapses
/// to `ApiKey` when the session-token cache is momentarily empty and
/// `XAI_API_KEY` is set, which demoted live OIDC sessions to non-refreshable
/// api-key mode and 401'd every prompt until restart. `model_byok` still
/// excludes genuine per-model BYOK, whose keys are not refreshable.
///
/// `Unknown` (BYOK status indeterminate — config currently unparseable, no
/// sampling config yet, or the per-model memo was cleared) must **not** demote
/// a live session to non-refreshable api-key mode: that re-sends the stale
/// buffered token on every turn and 401s with `bad-credentials` until restart
/// (the stale-token regression this gate addresses; fall back rather than
/// demote on `Unknown`). It refreshes when `endpoint_is_first_party` — the
/// request targets a first-party host (cli-chat-proxy / first-party API),
/// where sending the session token cannot leak to a third-party BYOK
/// endpoint. A definite `NotByok` always refreshes (it only ever routes to
/// the session endpoint); a definite `Byok` never does.
pub fn session_token_auth_gate(
    is_session_based_method: bool,
    model_byok: ModelByok,
    endpoint_is_first_party: bool,
) -> bool {
    is_session_based_method
        && match model_byok {
            ModelByok::NotByok => true,
            ModelByok::Byok => false,
            ModelByok::Unknown => endpoint_is_first_party,
        }
}

pub const AUTH_ERROR_SESSION_EXPIRED: &str =
    "Session expired. Set XAI_API_KEY and GROK_MODELS_BASE_URL, or run `/byok` in the TUI.";

pub const AUTH_ERROR_API_KEY: &str = "Authentication failed. Set XAI_API_KEY and GROK_MODELS_BASE_URL, or run `/byok` in the TUI.";

/// BYOK setup hint used when API-key credentials are missing.
pub const BYOK_SETUP_MESSAGE: &str = "Set XAI_API_KEY and GROK_MODELS_BASE_URL (or run `/byok` in the TUI) to configure your provider.";

/// Next ACP method id when `cached_token` cannot proceed.
///
/// Free/BYOK fork: always fall through to `xai.api_key` (OAuth is unavailable).
pub fn method_id_after_cached_token_unavailable(
    _has_external_api_key: bool,
    _preferred_method: Option<PreferredAuthMethod>,
) -> Option<&'static str> {
    Some(XAI_API_KEY_METHOD_ID)
}

/// Error when `preferred_method=api_key` but no key/BYOK credentials exist.
pub const PREFERRED_API_KEY_UNAVAILABLE: &str = "No API key configured. Set XAI_API_KEY and GROK_MODELS_BASE_URL, or run `/byok` in the TUI.";

/// Error when `preferred_method=oidc` but the session path cannot proceed.
pub const PREFERRED_OIDC_UNAVAILABLE: &str =
    "OIDC login is disabled in this build. Set XAI_API_KEY and GROK_MODELS_BASE_URL, or run `/byok`.";

pub const XAI_API_KEY_METHOD_ID: &str = "xai.api_key";
pub fn xai_api_key_auth_method() -> acp::AuthMethod {
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(
            acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID),
            "xai.api_key".to_string(),
        )
        .description(Some(format!(
            "{XAI_API_KEY_ENV_VAR} or api_key/env_key in config.toml"
        ))),
    )
}

pub const CACHED_TOKEN_AUTH_METHOD_ID: &str = "cached_token";
pub fn cached_token_auth_method() -> acp::AuthMethod {
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(
            acp::AuthMethodId::new(CACHED_TOKEN_AUTH_METHOD_ID),
            "cached_token".to_string(),
        )
        .description(Some("Cached token from ~/.grok/auth.json".to_string())),
    )
}

pub const GROK_COM_METHOD_ID: &str = "grok.com";

/// xAI OAuth2/OIDC auth. Method id `"grok.com"` kept for ACP wire-compat.
pub fn grok_com_auth_method(
    label: Option<&str>,
    has_auth_provider_command: bool,
) -> acp::AuthMethod {
    let name = label.unwrap_or("Grok");
    let meta = if has_auth_provider_command {
        let mut m = acp::Meta::new();
        m.insert("external_provider".to_owned(), serde_json::json!(true));
        Some(m)
    } else {
        None
    };
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(acp::AuthMethodId::new(GROK_COM_METHOD_ID), name.to_string())
            .description(Some(format!("Sign in with {name}")))
            .meta(meta),
    )
}

pub const OIDC_METHOD_ID: &str = "oidc";
pub fn oidc_auth_method(issuer: &str, label: Option<&str>) -> acp::AuthMethod {
    let name = label
        .map(|l| l.to_string())
        .unwrap_or_else(|| format!("Single sign-on ({})", issuer));
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(acp::AuthMethodId::new(OIDC_METHOD_ID), name.clone())
            .description(Some(format!("Sign in with {name}"))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::config::{Config, resolve_model_list};
    use agent_client_protocol as acp;
    use serial_test::serial;

    /// When API-key credentials are advertiseable, fall through from a dead
    /// `cached_token` to non-interactive `xai.api_key` (not browser OAuth).
    /// Covers the both-advertised case (`has_cached_token` true at initialize
    /// but session later missing/expired/legacy): advertise order still puts
    /// `xai.api_key` first, while `default_auth_method_id` prefers session;
    /// after session fails, this helper must still pick `xai.api_key`.
    #[test]
    fn after_cached_token_unavailable_prefers_api_key_when_advertiseable() {
        assert_eq!(
            method_id_after_cached_token_unavailable(true, None),
            Some(XAI_API_KEY_METHOD_ID),
        );
    }


    /// Free/BYOK fork: always fall through to xai.api_key (OAuth unavailable).
    #[test]
    fn after_cached_token_unavailable_always_api_key() {
        assert_eq!(
            method_id_after_cached_token_unavailable(false, None),
            Some(XAI_API_KEY_METHOD_ID),
        );
        assert_eq!(
            method_id_after_cached_token_unavailable(true, Some(PreferredAuthMethod::Oidc)),
            Some(XAI_API_KEY_METHOD_ID),
        );
        assert_eq!(
            method_id_after_cached_token_unavailable(false, Some(PreferredAuthMethod::ApiKey)),
            Some(XAI_API_KEY_METHOD_ID),
        );
    }

    /// Classifier matrix for all auth method variants.
    #[test]
    fn auth_method_kind_classifier_matrix() {
        let session_methods = [
            CACHED_TOKEN_AUTH_METHOD_ID,
            GROK_COM_METHOD_ID,
            OIDC_METHOD_ID,
        ];
        for method_id in session_methods {
            let id = acp::AuthMethodId::new(method_id);
            let kind = AuthMethodKind::from_id(&id);
            assert!(
                kind.is_session_based(),
                "{method_id}: kind must be session-based"
            );
            assert!(
                is_session_based_method(&id),
                "{method_id}: wrapper must agree"
            );
        }
        let api_id = acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID);
        let api_kind = AuthMethodKind::from_id(&api_id);
        assert!(!api_kind.is_session_based());
        assert!(api_kind.is_api_key());
        assert!(!is_session_based_method(&api_id));
        assert!(!is_session_based_method(&acp::AuthMethodId::new(
            "unknown-method"
        )));
    }

    fn default_inputs() -> AuthMethodsBuildInputs<'static> {
        AuthMethodsBuildInputs {
            has_external_api_key: false,
            has_cached_token: false,
            has_enterprise_oidc: false,
            enterprise_oidc_issuer: None,
            login_label: None,
            has_auth_provider_command: false,
            preferred_method: None,
        }
    }

    fn method_ids(built: &BuiltAuthMethods) -> Vec<&str> {
        built.methods.iter().map(|m| m.id().0.as_ref()).collect()
    }

    fn first_kind(methods: &[acp::AuthMethod]) -> Option<AuthMethodKind> {
        methods.first().map(|m| AuthMethodKind::from_id(m.id()))
    }

    /// Free/BYOK fork: build_auth_methods always advertises only xai.api_key.
    #[test]
    fn byok_fork_always_only_xai_api_key() {
        let cases = [
            default_inputs(),
            AuthMethodsBuildInputs {
                has_external_api_key: true,
                has_cached_token: true,
                has_enterprise_oidc: true,
                enterprise_oidc_issuer: Some("https://idp.example"),
                preferred_method: Some(PreferredAuthMethod::Oidc),
                ..default_inputs()
            },
            AuthMethodsBuildInputs {
                has_external_api_key: false,
                has_cached_token: false,
                preferred_method: Some(PreferredAuthMethod::ApiKey),
                ..default_inputs()
            },
        ];
        for inputs in cases {
            let built = build_auth_methods(inputs);
            assert_eq!(method_ids(&built), vec![XAI_API_KEY_METHOD_ID]);
            assert_eq!(
                first_kind(&built.methods),
                Some(AuthMethodKind::XaiApiKey)
            );
            assert_eq!(
                built.default_auth_method_id.as_ref().map(|id| id.0.as_ref()),
                Some(XAI_API_KEY_METHOD_ID)
            );
            assert!(!AuthMethodKind::from_id(built.methods[0].id()).needs_interactive_login());
        }
    }

    /// Free/BYOK fork: always advertise API-key auth regardless of kill-switch.
    #[test]
    fn should_advertise_always_true() {
        assert!(should_advertise_xai_api_key(true, std::iter::empty()));
        assert!(should_advertise_xai_api_key(false, std::iter::empty()));
    }

    #[test]
    fn session_token_auth_gate_matrix() {
        assert!(session_token_auth_gate(true, ModelByok::NotByok, false));
        assert!(!session_token_auth_gate(true, ModelByok::Byok, true));
        assert!(session_token_auth_gate(true, ModelByok::Unknown, true));
        assert!(!session_token_auth_gate(true, ModelByok::Unknown, false));
        assert!(!session_token_auth_gate(false, ModelByok::NotByok, true));
    }
}
