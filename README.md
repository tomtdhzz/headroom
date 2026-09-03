# headroom

**Per-model, rule-aware quota headroom for AI coding subscriptions.**

[English](README.md) · [简体中文](README.zh-CN.md)

Most limit trackers flatten your subscription into a few plan-level percentages.
`headroom` answers the question you actually have while coding:

> *For **each model**, under **its own** rules, how much can I still use — and if
> one is about to run out, what should I switch to?*

It reads usage through [`omp`](https://github.com/can1357/oh-my-pi), models each
provider's windows by **scope** (shared across all models vs. tier-specific), and
computes the *binding minimum* per model class — so a depleted Opus/Fable weekly
cap doesn't make you think the whole account is dead when Sonnet still has hours
left.

`headroom` is **read-only**: it never changes your `omp` config or switches
anything. It observes, forecasts, and alerts. (Auto-switching is a deliberate
non-goal for v0 — see [Limitations](#limitations).)

![headroom — interactive TUI (中文; press `l` to toggle 中/EN)](docs/assets/tui-zh.png)

## The problem it solves

If you code against a **Claude** (or **Codex**) subscription, your quota is not one
number. It is a rolling **5-hour window** plus **weekly caps**, metered by tokens —
and crucially a **shared pool for all models PLUS a separate cap for the premium
tier** (Claude's Opus / `fable`; Codex's `spark`).

So the same account, at the same moment, has *different* real headroom depending on
which model you pick. The classic failure: you burn the Opus weekly cap, Claude
starts refusing, and it feels like "my Claude is out" — when Sonnet still has hours
left. Existing trackers only show a flat per-account percentage, which hides the one
thing you need: **which model can I still use right now, and for how long?**

## The effect

`headroom` computes, per model, the **binding minimum** across every window that
constrains it, names the **bottleneck**, shows a reset countdown and a burn-rate
**ETA**, and — when a model is about to run out — tells you the cheaper model to
switch to, **before** you hit the wall:

```
Claude · f6* · max
  base   [██████████████░░░░░░]  72%  Claude 5 Hour           ↺4h12m  ~14h
  fable  [█░░░░░░░░░░░░░░░░░░░]   3%  Claude 7 Day (Fable)    ↺2d8h   ~36m

alerts
  CRIT  Claude/f6* — fable at 3% remaining (bottleneck: Claude 7 Day (Fable)) · switch to base (72% left)
```

Colored bar gauges, whole-row red on critical, an interactive TUI, and English or
中文 — all read-only (see [Limitations](#limitations)).

## Prerequisites

Platforms: macOS or Linux. Desktop notifications are macOS-only.

Two things must be installed:

1. **omp** ([Oh My Pi](https://github.com/can1357/oh-my-pi)) — on your `PATH` and
   authenticated. `headroom` shells out to `omp usage --json --redact`, so you need
   at least one logged-in provider:
   ```bash
   omp            # then run /login and sign in to Anthropic and/or OpenAI Codex
   omp usage      # sanity check: should print your windows
   ```
2. **Rust toolchain 1.88+** (to build from source). Install via rustup:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   . "$HOME/.cargo/env"     # or just open a new shell
   cargo --version          # expect 1.88 or newer
   ```

No other system libraries are needed — dependencies (serde, serde_json, anyhow,
ratatui, crossterm) are pure Rust and built by cargo.

## Install

### Prebuilt binary — recommended (no clone, no Rust)

macOS / Linux, one line:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/tomtdhzz/headroom/releases/latest/download/headroom-installer.sh | sh
```

Or download a `.tar.xz` for your platform from the
[latest release](https://github.com/tomtdhzz/headroom/releases/latest).

> macOS may quarantine an unsigned binary on first run. If Gatekeeper blocks it:
> `xattr -dr com.apple.quarantine "$(command -v headroom)"`.

### With cargo (needs the Rust toolchain)

```bash
cargo install --git https://github.com/tomtdhzz/headroom
```

### From source (fallback)

```bash
git clone https://github.com/tomtdhzz/headroom
cd headroom
cargo install --path .   # or: cargo run --release -- …
```

## Usage

```bash
# One-shot: render the model x window matrix and any alerts, then exit
headroom

# Limit to one provider
headroom --provider anthropic
headroom --provider openai-codex

# Tune thresholds (effective-remaining %)
headroom --warn 25 --critical 8

# Watch: poll on an interval; alert only on newly appeared/escalated conditions
headroom watch --interval 60

# Interactive TUI: bar gauges, ↑↓/jk select, r refresh, l 中/EN, q quit
headroom tui --interval 60

# Chinese display (auto-detected from locale; force with --lang)
headroom --lang zh
```

Options:

| Flag | Default | Meaning |
|---|---|---|
| `--provider <id>` | all | Limit to one provider (`anthropic`, `openai-codex`, …) |
| `--warn <pct>` | 20 | Warn when effective remaining ≤ this |
| `--critical <pct>` | 5 | Critical when effective remaining ≤ this |
| `--interval <secs>` | 60 | Poll interval in `watch` / `tui` auto-refresh |
| `--no-desktop` | off | Disable macOS desktop notifications |
| `--no-color` | auto | Disable ANSI color (also honors `NO_COLOR` and non-TTY) |
| `--lang <zh\|en>` | auto | Display language; auto-detects from `LANG`/`LC_*` |

## How it reads your limits

`omp usage --json` returns, per account, a list of limit windows each carrying a
`scope`:

- `scope.shared = true` → constrains **every** model (the 5h pool, the weekly cap).
- `scope.tier = "fable"` → constrains **only** that tier (the Opus weekly cap).

`headroom` derives, for each model class, its **binding windows** = all shared
windows ∪ that class's tier windows, and reports the minimum remaining as the
effective headroom, naming the bottleneck window. `status != "ok"` on any binding
window marks the class unavailable. No provider/tier names are hard-coded — the
mapping is driven entirely by `scope`.

## Architecture

A lightweight hexagon; the domain is pure and IO-free.

```
delivery/{cli,tui,i18n} ─┐
main ─────────┤→ app (Evaluator use case + ports)
              │        │
              │        └→ domain (LimitScope, LimitWindow, QuotaSnapshot,
              │                    BindingRule, Headroom, Forecast, Alert)
              └ adapters: omp_usage · history_file · clock · notify  ─┘ (implement ports)
```

- `domain/` — ubiquitous language and rules; zero IO; unit-tested off-network.
- `app/` — `Evaluator` (poll → record history → headroom → forecast → alerts) and
  the `UsageSource` / `HistoryStore` / `Notifier` / `Clock` ports.
- `adapters/` — `omp usage` anti-corruption mapping, XDG-cache history, system
  clock, stderr + macOS desktop notifier.
- `delivery/` — CLI bar-gauge renderer and interactive `tui` (ratatui), localized via `i18n` (`--lang`, auto-detected). Swapping delivery never touches the core.

See [`docs/prd/PRD.md`](docs/prd/PRD.md) and
[`docs/tech-design/tech-design.md`](docs/tech-design/tech-design.md).

## Privacy

Only quota-shaped data touches disk. Local history under
`$XDG_CACHE_HOME/headroom/history.json` (or `~/.cache/headroom/history.json`)
stores `provider|account-prefix|window-id → (timestamp, remaining%)` — never
credentials, emails, org identity, or raw provider responses. The account id is
the already-redacted prefix from `omp --redact`.

## Test

```bash
cargo test          # unit + contract + end-to-end (uses redacted fixtures)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Limitations

- **Read-only.** v0 observes and alerts; it does not auto-switch models/accounts.
  Acting on alerts (generating `omp` `modelRoles` / `retry.fallbackChains`, or
  enabling `usageAwareFallback`) is planned for v1.
- **Tier granularity.** `omp` exposes usage per tier, not per exact model, so
  headroom is computed per tier/class. Per-model attribution inside a shared pool
  (e.g. Sonnet vs. Haiku) needs session accounting — planned for v2.
- **Forecast needs history.** ETA appears only after ≥2 differing samples; a
  single reading shows `-`.
- **Windows may change.** Provider limit structures change often; the `scope`-driven
  mapping avoids hard-coding tiers, but new shapes may need a parser update.

## Roadmap

Deferred by design; tracked so the read-only core stays honest.

- **v1 — act on alerts (on hold).** Turn a suggestion into action: generate/patch
  omp `modelRoles` and `retry.fallbackChains`, and optionally enable
  `retry.usageAwareFallback` for request-time downgrade. Opt-in, never silent.
- **v2 — per-exact-model attribution.** Break shared-pool burn down to individual
  models (e.g. Sonnet vs. Haiku) via omp session accounting.
- **v2 — pooled multi-account capacity view** using omp's `capacity` block.
- **Maybe — absolute `≈Nk` estimates** behind a user-supplied per-window budget
  (omp only reports percentages today, so any absolute is an explicit estimate).

## License

MIT — see [LICENSE](LICENSE).

## Disclaimer

Independent project, not affiliated with or endorsed by Anthropic, OpenAI, or the
`omp` / Oh My Pi authors. Provider interfaces and subscription limits may change.
