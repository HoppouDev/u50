# Harness-Score Compliance Plan

**Baseline:** L1 · Documented — 65/108 (60%) — gate: maturity — detected harness: Codex
**Source:** `tmp/HARNESS_REPORT.md` + [official guide](https://paladini.github.io/harness-score/guide/measure-and-improve)
**Goal:** L2 via Phase 2; full sweep targets **105/108 (97%)** (108/108 requires an unused MCP config — deliberately skipped).

## 1. Current state

| Dimension | Score | Status |
|---|---|---|
| Context & Guides | 19/20 (95%) | One gap: CTX-07 |
| Skills & Commands | 0/17 (0%) | All four checks failing |
| Hooks & Guardrails | 0/14 (0%) | All five checks failing |
| Sensors & Feedback | 15/20 (75%) | One gap: SNS-02 |
| CI Feedback | 11/14 (79%) | One gap: CI-04 |
| Hygiene & Safety | 20/23 (87%) | One gap: HYG-08 (bonus) |

**L2 gate:** skills ≥ 30% (of 17) or hooks ≥ 30% (of 14).
- Skills: need ≥ 5.1 pts → e.g. SKL-01 (4) + SKL-04 (2) = 6 pts crosses the gate.
- Hooks: need ≥ 4.2 pts → HKS-01 (4) alone is 28.6% (not enough); HKS-01 + HKS-02 (6) crosses it.

## 2. Failing checks (43 pts available)

| Check | Pts | Fix (per guide) |
|---|---|---|
| CTX-07 README present | 1 | Add a root `README.md` — first orientation doc for humans and agents. |
| SKL-01 at least one skill | 4 | `SKILL.md` under `.cursor/skills/<name>/`, `.claude/skills/<name>/`, or `.agents/skills/<name>/`. |
| SKL-02 skill name + description | 3 | Frontmatter with `name:` and `description:` on every skill. |
| SKL-03 explicit workflows/commands | 3 | Files under `.cursor/commands/`, `.agents/workflows/`, `.claude/commands/`, etc. |
| SKL-04 trigger-worthy descriptions | 2 | Descriptions ≥ 40 chars, written as trigger conditions ("Use when…"). |
| AGT-01 custom subagent | 3 | Subagent file under `.cursor/agents/`, `.claude/agents/`, or `.opencode/agents/`. |
| AGT-02 subagent name + description | 2 | `name:` and `description:` frontmatter on every subagent. |
| HKS-01 hooks config valid JSON | 4 | `.cursor/hooks.json` or `.claude/settings.json` (`hooks` key), parses, non-empty. |
| HKS-02 structurally valid events | 2 | Non-empty event map, valid handlers, required vendor metadata (e.g. Cursor `version`). |
| HKS-03 gate hook | 4 | `PreToolUse` (Claude Code) or `beforeShellExecution`/`beforeMCPExecution`/`preToolUse` (Cursor) returning allow/deny/ask for destructive ops. |
| HKS-04 feedback hook | 2 | `PostToolUse` (Claude Code) or `afterFileEdit`/`postToolUse` (Cursor) — e.g. format-and-lint on edit. |
| HKS-05 hook scripts committed | 2 | Every repo-local path in command handlers resolves to a committed file. |
| SNS-02 linter configured | 5 | eslint/biome, ruff, golangci-lint, rubocop, **clippy.toml**, or equivalent. |
| CI-04 pre-commit checks | 3 | husky + lint-staged, `pre-commit`, or lefthook. |
| HYG-08 MCP env interpolation | 3 | Bonus check; needs a valid MCP config with `${ENV_VAR}` for credentials. No MCP setup → earns nothing (same as now). |

## 3. Phased plan

### Phase 1 — Quick wins (6 pts, ~1 h)

1. **CTX-07 (1):** root `README.md` — project purpose, build/test/lint commands, pointer to `AGENTS.md` and the per-crate docs.
2. **SNS-02 (5):** root `clippy.toml` capturing the workspace's pedantic lint policy. CI already runs `cargo clippy -Dwarnings` (CI-03 ✅); this makes the linter *configuration* discoverable. [I] The report lists `clippy.toml` as a recognized linter config; verify by re-running the scanner.

### Phase 2 — Skills & workflows (12 pts, ~½ day) → **L2 gate crossed**

Directory choice: `.agents/` (Codex was the detected harness; `.agents/skills/` and `.agents/workflows/` are both recognized). [I]

1. **SKL-01/02/04 (9):** `.agents/skills/release/SKILL.md` packaging the repo's most repeated procedure (the build → fmt → clippy -Dwarnings → test → commit → push cadence), with frontmatter:
   ```yaml
   ---
   name: release
   description: >-
     Use when committing, verifying, or shipping changes to u50; covers the
     fmt/clippy/test gate, conventional commits, and push-and-monitor-CI steps.
   ---
   ```
   Description ≥ 40 chars, trigger-style (SKL-04).
2. **SKL-03 (3):** `.agents/workflows/release.md` — explicit entry point for the same on-demand workflow.

### Phase 3 — Hooks & guardrails (14 pts, ~½ day)

Harness-surface choice: the scanner only recognizes `.cursor/hooks.json` or `.claude/settings.json` for hooks, so use **`.claude/settings.json`** (Claude Code event catalog is fully documented in the guide). [I]

1. **HKS-01/02 (6):** `.claude/settings.json` with a non-empty `hooks` map and structurally valid handlers (Claude handlers may be `command`, `http`, `mcp_tool`, `prompt`, or `agent` — each with required fields).
2. **HKS-03 (4):** `PreToolUse` gate hook — deny/ask for destructive shell commands (force-push, `rm -rf`, branch deletion). Prose rules are requests; gates are facts.
3. **HKS-04 (2):** `PostToolUse` feedback hook — run `cargo fmt` / quick clippy check on edited `.rs` files for in-session feedback.
4. **HKS-05 (2):** commit all referenced scripts under `.claude/hooks/` — a missing script fails open everywhere but the author's machine.

### Phase 4 — Pre-commit (3 pts, ~1–2 h)

**CI-04 (3):** lefthook (single static binary, fits the zero-Python-dependency spirit of the repo) running `cargo fmt --check` and `cargo clippy` on staged files. [I] Verify scanner recognition of the chosen tool.

**Running total after Phases 1–4: 100/108 (93%).**

### Phase 5 — Subagent (5 pts) → **105/108 (97%)**

**AGT-01/02 (5):** `.claude/agents/gate-runner.md` — a purpose-built delegate that runs the quality gates (fmt → clippy `-Dwarnings` → build → test → goldens) and reports pass/fail with verbatim output, so the primary agent can delegate verification instead of doing it inline (delegation is already the working pattern in this repo). Frontmatter: `name: gate-runner`, `description:` (≥ 40 chars, trigger-style: "Use when a change needs independent verification without polluting the main context …").

### Phase 6 — Optional remainder (3 pts)

**HYG-08 (3):** only if an MCP config is ever added; must use `${ENV_VAR}` interpolation with variables documented in `.env.example`. **Deliberately skipped** — the repo needs no MCP setup, and this bonus check awards nothing without one. Skipping it caps the score at 105/108; 108/108 requires an MCP config we do not genuinely use.

## 4. Verification cadence

After each phase: `npx harness-score`, compare against the phase's expected points, and confirm no regression on the 29 already-passing checks. Run `--min-level 2` in CI once L2 is reached so the level cannot silently regress.

## 5. Explicitly out of scope

- No code changes to `u50_check`/`u50_style`/`u50_submit` — this plan touches only harness/repo-meta files.
- No merge to `main` of feature work; harness artifacts land via their own commits on the current branch per cadence.
