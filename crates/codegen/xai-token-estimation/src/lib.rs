//! Pure shared token-estimation primitives.
//!
//! This crate is the single source of truth for the bytes/4 heuristic and the
//! derived-display arithmetic that `/context`, `/session-info`, the auto-compact
//! gates, the preflight overflow check, and every client renderer use to talk
//! about context-window usage.

/// Bytes per token under the rough character-based heuristic.
pub const BYTES_PER_TOKEN: u64 = 4;

/// Per-image approximate token cost when summing
/// low-resolution image patches.
pub const IMAGE_TOKEN_ESTIMATE: u64 = 765;

/// Default threshold (percent of budget) at which a soft nudge is injected.
pub const BUDGET_NUDGE_PERCENT: u8 = 80;

/// Bytes/4 estimate of a string's token count.
#[inline]
pub fn estimate_tokens(s: &str) -> u64 {
    (s.len() as u64) / BYTES_PER_TOKEN
}

/// Inverse of [`estimate_tokens`]: convert a token budget into a character
/// budget. Used by skill discovery to size text passages against the model's
/// context window.
#[inline]
pub fn estimate_chars(tokens: u64) -> u64 {
    tokens.saturating_mul(BYTES_PER_TOKEN)
}

/// Token estimate for `image_count` images at [`IMAGE_TOKEN_ESTIMATE`] each.
#[inline]
pub fn estimate_image_tokens(image_count: u64) -> u64 {
    image_count.saturating_mul(IMAGE_TOKEN_ESTIMATE)
}

/// Usage percentage as `f64`, clamped to `100.0`. Returns `0.0` when
/// `total == 0`.
#[inline]
pub fn usage_percentage(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((used as f64) / (total as f64) * 100.0).min(100.0)
    }
}

/// Usage percentage rounded to `u8`, clamped to `100`.
#[inline]
pub fn usage_percentage_u8(used: u64, total: u64) -> u8 {
    usage_percentage(used, total).round() as u8
}

/// Integer-arithmetic (truncating) usage percentage, clamped to `100`.
///
/// Differs from [`usage_percentage_u8`] in two ways: no `f64` round-trip,
/// and the result is **truncated** (not rounded).
///
/// Returns `u8` because the result is bounded to `100`. Saturates on
/// overflow via `saturating_mul`.
#[inline]
pub fn usage_percentage_truncated_u8(used: u64, total: u64) -> u8 {
    if total == 0 {
        0
    } else {
        ((used.saturating_mul(100) / total).min(100)) as u8
    }
}

/// `total - used`, saturating at zero. The "free" portion of the context
/// window for `/context` rendering.
#[inline]
pub fn free_tokens(total: u64, used: u64) -> u64 {
    total.saturating_sub(used)
}

/// True when `used >= context_window * threshold_percent / 100`. Returns
/// `false` for `context_window == 0` so callers do not have to special-case
/// missing windows. Computed in integer arithmetic to match the existing
/// auto-compact gate semantics.
#[inline]
pub fn exceeds_threshold(used: u64, context_window: u64, threshold_percent: u8) -> bool {
    if context_window == 0 {
        return false;
    }
    used.saturating_mul(100) >= context_window.saturating_mul(threshold_percent as u64)
}

/// True when `used * 100 >= context_window * threshold_percent - headroom * 100`,
/// the scaled form of [`exceeds_threshold`] minus a token headroom.
/// Returns `false` for `context_window == 0`.
#[inline]
pub fn exceeds_threshold_with_headroom(
    used: u64,
    context_window: u64,
    threshold_percent: u8,
    headroom: u64,
) -> bool {
    if context_window == 0 {
        return false;
    }
    used.saturating_mul(100)
        >= context_window
            .saturating_mul(threshold_percent as u64)
            .saturating_sub(headroom.saturating_mul(100))
}

/// Severity of a budget nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetNudge {
    /// No nudge needed.
    None,
    /// Soft nudge: approaching budget (e.g. 80% consumed).
    Approaching,
    /// Hard nudge: budget exceeded — wrap up.
    Exceeded,
}

/// Tracks cumulative output tokens against a user-specified budget.
///
/// Used to detect diminishing returns and nudge the agent near budget limits.
#[derive(Debug, Clone, Default)]
pub struct TokenBudget {
    /// Total budget in tokens. `None` means no budget is set.
    pub total: Option<u64>,
    /// Cumulative output tokens consumed this session.
    pub consumed: u64,
    /// Soft-nudge threshold as a percent of total (default 80).
    pub nudge_percent: u8,
    /// Running count of turns since the last useful result (for diminishing returns).
    pub turns_since_useful: u32,
}

impl TokenBudget {
    /// Create a new budget with the given total and default nudge percent.
    pub fn new(total: u64) -> Self {
        Self {
            total: Some(total),
            consumed: 0,
            nudge_percent: BUDGET_NUDGE_PERCENT,
            turns_since_useful: 0,
        }
    }

    /// Parse a budget string like `"+500k"`, `"500000"`, or `"1m"`.
    pub fn parse(s: &str) -> Option<u64> {
        let s = s.trim().trim_start_matches('+').to_lowercase();
        if s.is_empty() {
            return None;
        }
        let (num_str, mult) = if let Some(rest) = s.strip_suffix('k') {
            (rest, 1_000u64)
        } else if let Some(rest) = s.strip_suffix('m') {
            (rest, 1_000_000u64)
        } else {
            (s.as_str(), 1u64)
        };
        let num: f64 = num_str.parse().ok()?;
        if !num.is_finite() || num < 0.0 {
            return None;
        }
        Some((num * mult as f64) as u64)
    }

    /// Record output tokens consumed after a turn.
    pub fn record_output(&mut self, tokens: u64) {
        self.consumed = self.consumed.saturating_add(tokens);
    }

    /// Mark that a useful result was produced (resets diminishing-returns counter).
    pub fn mark_useful(&mut self) {
        self.turns_since_useful = 0;
    }

    /// Mark that a turn completed without a useful result.
    pub fn mark_unuseful(&mut self) {
        self.turns_since_useful = self.turns_since_useful.saturating_add(1);
    }

    /// Remaining tokens in the budget, or `None` if no budget is set.
    pub fn remaining(&self) -> Option<u64> {
        self.total.map(|t| free_tokens(t, self.consumed))
    }

    /// Percent of budget consumed, or `None` if no budget is set.
    pub fn consumed_percent(&self) -> Option<u8> {
        self.total
            .map(|t| usage_percentage_truncated_u8(self.consumed, t))
    }

    /// Compute the nudge severity based on current consumption.
    pub fn nudge(&self) -> BudgetNudge {
        let Some(total) = self.total else {
            return BudgetNudge::None;
        };
        if total == 0 {
            return BudgetNudge::None;
        }
        if self.consumed >= total {
            return BudgetNudge::Exceeded;
        }
        if exceeds_threshold(self.consumed, total, self.nudge_percent) {
            return BudgetNudge::Approaching;
        }
        // Diminishing returns: 3+ turns without useful results near 50%+ budget.
        if self.turns_since_useful >= 3
            && exceeds_threshold(self.consumed, total, 50)
        {
            return BudgetNudge::Approaching;
        }
        BudgetNudge::None
    }

    /// Human-readable nudge message, or `None` if no nudge is needed.
    pub fn nudge_message(&self) -> Option<String> {
        match self.nudge() {
            BudgetNudge::None => None,
            BudgetNudge::Approaching => {
                let remaining = self.remaining().unwrap_or(0);
                Some(format!(
                    "You have ~{remaining} tokens remaining in your budget. Prioritize delivering results."
                ))
            }
            BudgetNudge::Exceeded => Some(
                "Budget exceeded. Wrap up and deliver results.".to_string(),
            ),
        }
    }

    /// Clear the budget.
    pub fn clear(&mut self) {
        self.total = None;
        self.consumed = 0;
        self.turns_since_useful = 0;
    }

    /// Status summary for `/budget` display.
    pub fn status_message(&self) -> String {
        match self.total {
            None => "No token budget set. Use `/budget <amount>` (e.g. `/budget 500k`).".to_string(),
            Some(total) => {
                let pct = self.consumed_percent().unwrap_or(0);
                let remaining = free_tokens(total, self.consumed);
                format!(
                    "Budget: {consumed}/{total} tokens ({pct}% used, ~{remaining} remaining)",
                    consumed = self.consumed,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_is_bytes_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abc"), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens(&"x".repeat(4000)), 1000);
    }

    #[test]
    fn estimate_chars_is_inverse() {
        assert_eq!(estimate_chars(0), 0);
        assert_eq!(estimate_chars(1), 4);
        assert_eq!(estimate_chars(1000), 4000);
    }

    #[test]
    fn estimate_image_tokens_uses_constant() {
        assert_eq!(estimate_image_tokens(0), 0);
        assert_eq!(estimate_image_tokens(1), IMAGE_TOKEN_ESTIMATE);
        assert_eq!(estimate_image_tokens(3), 3 * IMAGE_TOKEN_ESTIMATE);
    }

    #[test]
    fn usage_percentage_clamps_and_handles_zero_total() {
        assert_eq!(usage_percentage(0, 0), 0.0);
        assert_eq!(usage_percentage(50, 100), 50.0);
        assert_eq!(usage_percentage(150, 100), 100.0);
        assert_eq!(usage_percentage(100, 0), 0.0);
    }

    #[test]
    fn usage_percentage_u8_rounds() {
        assert_eq!(usage_percentage_u8(0, 100), 0);
        assert_eq!(usage_percentage_u8(50, 100), 50);
        assert_eq!(usage_percentage_u8(99, 100), 99);
        assert_eq!(usage_percentage_u8(12_700, 256_000), 5);
        assert_eq!(usage_percentage_u8(150, 100), 100);
    }

    #[test]
    fn usage_percentage_u8_rounds_half_up() {
        assert_eq!(usage_percentage_u8(85, 200), 43);
        assert_eq!(usage_percentage_u8(7, 8), 88);
    }

    #[test]
    fn usage_percentage_truncated_u8_clamps_and_handles_zero_total() {
        assert_eq!(usage_percentage_truncated_u8(0, 0), 0);
        assert_eq!(usage_percentage_truncated_u8(50, 100), 50);
        assert_eq!(usage_percentage_truncated_u8(150, 100), 100);
        assert_eq!(usage_percentage_truncated_u8(u64::MAX, 1), 100);
    }

    #[test]
    fn usage_percentage_truncated_u8_truncates_does_not_round() {
        assert_eq!(usage_percentage_truncated_u8(85, 200), 42);
        assert_eq!(usage_percentage_truncated_u8(7, 8), 87);
    }

    #[test]
    fn free_tokens_saturates() {
        assert_eq!(free_tokens(100, 30), 70);
        assert_eq!(free_tokens(100, 100), 0);
        assert_eq!(free_tokens(100, 200), 0);
    }

    #[test]
    fn exceeds_threshold_matches_integer_pct() {
        assert!(!exceeds_threshold(50, 100, 85));
        assert!(exceeds_threshold(85, 100, 85));
        assert!(exceeds_threshold(99, 100, 85));
        assert!(!exceeds_threshold(50, 0, 85));
    }

    #[test]
    fn token_budget_parse() {
        assert_eq!(TokenBudget::parse("500k"), Some(500_000));
        assert_eq!(TokenBudget::parse("+500k"), Some(500_000));
        assert_eq!(TokenBudget::parse("1m"), Some(1_000_000));
        assert_eq!(TokenBudget::parse("1000"), Some(1000));
        assert_eq!(TokenBudget::parse(""), None);
        assert_eq!(TokenBudget::parse("abc"), None);
    }

    #[test]
    fn token_budget_nudge_approaching() {
        let mut b = TokenBudget::new(1000);
        b.record_output(800);
        assert_eq!(b.nudge(), BudgetNudge::Approaching);
        assert!(b.nudge_message().unwrap().contains("remaining"));
    }

    #[test]
    fn token_budget_nudge_exceeded() {
        let mut b = TokenBudget::new(1000);
        b.record_output(1000);
        assert_eq!(b.nudge(), BudgetNudge::Exceeded);
    }

    #[test]
    fn token_budget_no_budget() {
        let b = TokenBudget::default();
        assert_eq!(b.nudge(), BudgetNudge::None);
        assert!(b.nudge_message().is_none());
    }

    #[test]
    fn token_budget_diminishing_returns() {
        let mut b = TokenBudget::new(1000);
        b.record_output(500);
        b.mark_unuseful();
        b.mark_unuseful();
        b.mark_unuseful();
        assert_eq!(b.nudge(), BudgetNudge::Approaching);
    }
}
