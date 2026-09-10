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

`headroom`'s **monitoring is read-only**: it never touches your `omp` config
while observing, forecasting, and alerting. It also ships an **opt-in switcher**
(Clash-style "policy groups") to manually pin roles to models — the one action
that writes config, and only to `modelRoles`, verified by read-back (see
[Switching](#switching-models--platforms)). Auto-switching remains a non-goal
(see [Limitations](#limitations)).

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
**ETA**, tells you the cheaper model to switch to **before** you hit the wall,
and — for a constrained model — the **exact local time its window refreshes**, a
proactive *"resume at"* alarm so you can plan around the reset instead of
passively waiting it out:

```
Claude · f6* · max
  base   [██████████████░░░░░░]  72%  Claude 5 Hour           ↺4h12m  ~14h
  fable  [█░░░░░░░░░░░░░░░░░░░]   3%  Claude 7 Day (Fable)    ↺2d8h   ~36m

alerts
  CRIT  Claude/f6* — fable at 3% remaining (bottleneck: Claude 7 Day (Fable)) · switch to base (72% left) · resets Fri 09:00 (↺2d8h)
```

Colored bar gauges, whole-row red on critical, an interactive TUI, and English or
中文 — all read-only (see [Limitations](#limitations)). In `watch` mode a
refreshed class also pings once ("available now") the moment its window resets.

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

### Homebrew (macOS / Linux)

```bash
brew install tomtdhzz/tap/headroom
```

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

# Interactive TUI: bar gauges, ↑↓/jk select, r refresh, l 中/EN, q quit;
# press s for the "policy group" switch pane
headroom tui --interval 60

# Chinese display (auto-detected from locale; force with --lang)
headroom --lang zh
```

### Switching models / platforms

Treat omp's roles (`default`/`plan`/`slow`/`smol`/`advisor`) as Clash "policy
groups" and concrete models as "nodes", then pin them manually:

```bash
# Show which model each role is pinned to (unset = auto, omp chooses)
headroom roles

# Pin a role to a model (fuzzy match; a non-unique query lists candidates,
# errors, and writes nothing)
headroom use default anthropic/claude-opus-4-8
headroom use smol   haiku

# Clear a pin, back to omp's default
headroom clear default

# List models with price + capabilities + task recommendations
headroom models --provider anthropic --filter opus
```

`use` / `clear` (and the TUI "apply") are the **only** actions that write
config: they touch omp's `modelRoles` and only that, and verify by read-back.
A switch takes effect on the **next omp session** (same as cc-switch; it never
disturbs a running one). In the TUI switch pane, each node's color reflects that
provider's **live headroom** (green/yellow/red), the current pin is marked `●`,
and the `◎ Auto` row clears the pin (the URLTest analog — hand it back to omp).

**See cost and fit while switching.** In the TUI switch pane each node shows its
**price** (output $/M tokens; green=cheap, gray=mid, magenta=pricey) and
capability tags (`👁` reads images, `🧠` reasoning); the highlighted node's full
price (in/out) and context size appear below, and a **recommendation** line
suggests the best model per task axis — `💡 cheapest / vision / reasoning /
long-ctx` — all computed from real `omp models` fields. Press `t` to sort by
price so the cheaper escape model surfaces on a quota alert.

> Note: the price is omp's **API list price** ($/M tokens) — a *relative* cost
> signal. On a subscription, switching models produces no dollar bill; real quota
> burn is the pane's color (provider live headroom). `👁` means the model can
> *read* images (vision), not generate them — these are coding models.

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

- **Monitoring is read-only; switching is explicit.** Observing and alerting
  never write config. Manual switching (`use`/`clear` or a TUI apply) is the only
  write path — opt-in, verified by read-back, effective next session, touching
  only omp's `modelRoles`. It still does **not** auto-switch (use omp's native
  `retry.usageAwareFallback` / `fallbackChains` for at-the-wall downgrade).
- **Tier granularity.** `omp` exposes usage per tier, not per exact model, so
  headroom is computed per tier/class. Per-model attribution inside a shared pool
  (e.g. Sonnet vs. Haiku) needs session accounting — planned for v2. The switch
  pane's per-model **price** is omp's API list price (a relative cost signal), not
  subscription-quota consumption — omp exposes no model→tier map to attribute that.
- **Forecast needs history.** ETA appears only after ≥2 differing samples; a
  single reading shows `-`.
- **Windows may change.** Provider limit structures change often; the `scope`-driven
  mapping avoids hard-coding tiers, but new shapes may need a parser update.
- **Refresh-alarm timezone.** The "resets at HH:MM" alarm renders in your local
  wall-clock using the current UTC offset (read once from the OS). A reset that
  falls across a daylight-saving boundary within the horizon may be off by an
  hour; the relative countdown (`↺`) is always exact.

The core stays honest: monitoring read-only, switching explicit and verified.

- **v1 — manual switching (shipped).** Clash "policy group"-style role→model
  pinning: `headroom roles` / `use` / `clear` and the TUI `s` switch pane, writing
  omp `modelRoles` with read-back verification.
- **Next — one-key actionable alerts.** Offer a "switch to the suggested model"
  action right next to an alert; optionally enable omp's
  `retry.usageAwareFallback` for request-time downgrade.
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
