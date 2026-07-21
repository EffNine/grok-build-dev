---
name: verify
description: >
  Verify recent changes by reviewing the diff, running builds/tests, and
  checking correctness. Use when the user asks to verify, check work,
  self-verify, or runs /verify. Claude Code–compatible alias for /check-work.
argument-hint: "[focus area]"
metadata:
  short-description: "Verify changes (alias for /check-work)"
---

# /verify — Self-Verification

This is the Claude Code–compatible alias for `/check-work`.

Execute the **same verification workflow** as the bundled `check-work` skill:

1. Read `~/.grok/skills/check-work/SKILL.md` (the extracted bundled copy) and
   follow its instructions exactly.
2. If that path is unavailable, follow the check-work skill content already
   in your skill context, or fall back to the workflow summarized below.
3. Pass through any focus area the user provided after `/verify`.

## Fallback workflow (if check-work content is unavailable)

1. Spawn a verification subagent (`general-purpose`, not background) whose
   description starts with `[checking my work]`.
2. Have it review the session work, inspect the current git diff, run the
   project's build/test/lint commands from AGENTS.md / README, and end with
   exactly `VERDICT: PASS` or `VERDICT: FAIL`.
3. If FAIL, fix the issues and repeat (up to 3 times). If PASS, summarize
   what was confirmed and stop.

Prefer the full check-work skill instructions whenever they are available;
this fallback is only a safety net.
