---
name: init
description: >
  Bootstrap project instructions by creating a root AGENTS.md. Use when the
  user runs /init, asks to set up project rules, generate AGENTS.md, or
  initialize Claude/Grok coding-agent instructions for a repository.
disable-model-invocation: true
metadata:
  short-description: "Create AGENTS.md project instructions"
---

# /init — Bootstrap Project Rules

Analyze this repository and create a root `AGENTS.md` with concise,
project-specific instructions for Grok Build (and other AGENTS.md-compatible
coding agents).

## Goals

Write an `AGENTS.md` that a coding agent can follow on every session without
the user restating:

- How to build, test, lint, and run the project
- Language / framework conventions that matter
- Repo layout gotchas and non-obvious constraints
- Safety / process rules the team cares about

Keep it short and actionable. Prefer a few high-signal sections over a long
essay.

## Steps

1. **Locate the write target**
   - Prefer the git repository root (`git rev-parse --show-toplevel`).
   - If not inside a git repo, use the current working directory.
   - The file to create is `<root>/AGENTS.md`.

2. **Check what already exists**
   - If `AGENTS.md` (or `Agents.md` / `AGENT.md`) already exists at that root:
     - Do **not** overwrite it.
     - Summarize what is already there.
     - Offer a short list of concrete improvements the user can choose to
       apply (and only edit if they ask).
     - Stop after the summary unless the user explicitly asks you to update
       the file.
   - Note related files for context only: `CLAUDE.md`, `.claude/CLAUDE.md`,
     `README.md`, `.grok/rules/`, `.claude/rules/`. Grok already loads
     Claude-compatible instruction files when present; still prefer creating
     a native `AGENTS.md` when it is missing.

3. **Survey the project** (read, do not guess)
   - Root `README.md` / docs for setup and workflows
   - Package / build manifests (`Cargo.toml`, `package.json`, `pyproject.toml`,
     `go.mod`, `Makefile`, etc.)
   - CI configs (`.github/workflows/`, etc.) for the real test/lint commands
   - Existing instruction files listed above
   - A light scan of top-level source layout

4. **Write `AGENTS.md`**
   - Create the file only when it does not already exist.
   - Use clear markdown headings.
   - Include only sections that are grounded in what you found. Typical
     sections:
     - `# Project overview` (1–3 sentences)
     - `## Build & test` (exact commands)
     - `## Conventions` (only non-obvious rules)
     - `## Architecture / layout` (only if it prevents wrong edits)
     - `## Safety / process` (secrets, generated files, do-not-touch paths)
   - Do **not** invent commands or conventions. If something is unclear, omit
     it or mark it as unknown.
   - Do **not** dump large directory trees, dependency lists, or generated
     API docs into the file.

5. **Confirm to the user**
   - Say where the file was written.
   - Briefly list the sections included.
   - Remind them they can edit `AGENTS.md` anytime, and that deeper
     directories may add their own `AGENTS.md` files for local overrides.

## Tone

Be practical and specific. The file is for agents, not marketing copy.
