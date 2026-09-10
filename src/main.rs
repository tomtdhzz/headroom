//! Composition root: parse args, wire adapters into the use case, render, and
//! (optionally) watch on an interval.

use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Result};

use headroom::adapters::{
    local_utc_offset_seconds, CliNotifier, FileHistoryStore, OmpModelControl, OmpUsageSource,
    Recovered, SystemClock,
};
use headroom::app::ports::Clock;
use headroom::app::{Evaluator, Outcome, Switcher};
use headroom::delivery::cli::{render, Palette};
use headroom::delivery::switch::{render_models, render_outcome, render_roles};
use headroom::delivery::Locale;
use headroom::domain::{Alert, AlertLevel, Role, Thresholds};

const HELP: &str = "\
headroom — per-model quota headroom for AI coding subscriptions (via omp)

USAGE:
    headroom [OPTIONS]                 One-shot per-model headroom + alerts
    headroom watch [OPTIONS]           Poll on an interval, notify on change
    headroom tui [OPTIONS]             Interactive TUI (press `s` for switch pane)
    headroom roles                     Show role→model pins (the policy groups)
    headroom use <role> <model>        Pin a role to a model (fuzzy match)
    headroom clear <role>              Clear a role's pin (back to omp default)
    headroom models [--filter <s>]     List models with price + caps + recommendations

ROLES: default · plan · slow · smol · advisor

OPTIONS:
    --provider <id>     Limit to one provider (e.g. anthropic, openai-codex)
    --warn <pct>        Warn threshold, effective remaining % (default 20)
    --critical <pct>    Critical threshold, effective remaining % (default 5)
    --interval <secs>   Poll interval in watch / tui auto-refresh (default 60)
    --no-desktop        Disable macOS desktop notifications
    --no-color          Disable ANSI color
    --lang <zh|en>      Display language (default: auto-detect from locale)
    -h, --help          Print this help

Monitoring reads `omp usage --json --redact` (read-only). Switching writes omp
`modelRoles` only on explicit `use`/`clear` or a TUI apply, verified by read-back.
";

/// The selected subcommand.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Show,
    Watch,
    Tui,
    Roles,
    Use,
    Clear,
    Models,
}

struct Args {
    provider: Option<String>,
    warn: u8,
    critical: u8,
    interval: u64,
    cmd: Cmd,
    role: Option<String>,
    query: Option<String>,
    desktop: bool,
    color: Option<bool>,
    lang: Option<Locale>,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            provider: None,
            warn: 20,
            critical: 5,
            interval: 60,
            cmd: Cmd::Show,
            role: None,
            query: None,
            desktop: true,
            color: None,
            lang: None,
        }
    }
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "watch" => args.cmd = Cmd::Watch,
            "tui" => args.cmd = Cmd::Tui,
            "roles" => args.cmd = Cmd::Roles,
            "use" => {
                args.cmd = Cmd::Use;
                args.role = Some(next_value(&mut it, "use <role>")?);
                args.query = Some(next_value(&mut it, "use <role> <model>")?);
            }
            "clear" => {
                args.cmd = Cmd::Clear;
                args.role = Some(next_value(&mut it, "clear <role>")?);
            }
            "models" => args.cmd = Cmd::Models,
            "--filter" => args.query = Some(next_value(&mut it, "--filter")?),
            "--provider" => args.provider = Some(next_value(&mut it, "--provider")?),
            "--warn" => args.warn = parse_pct(&next_value(&mut it, "--warn")?, "--warn")?,
            "--critical" => {
                args.critical = parse_pct(&next_value(&mut it, "--critical")?, "--critical")?
            }
            "--interval" => {
                let v = next_value(&mut it, "--interval")?;
                args.interval = v
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--interval expects seconds, got `{v}`"))?;
                if args.interval == 0 {
                    bail!("--interval must be greater than 0");
                }
            }
            "--no-desktop" => args.desktop = false,
            "--no-color" => args.color = Some(false),
            "--lang" => args.lang = Some(parse_lang(&next_value(&mut it, "--lang")?)?),
            other => bail!("unknown argument `{other}` (try --help)"),
        }
    }
    Ok(Some(args))
}

fn next_value(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    it.next()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn parse_lang(v: &str) -> Result<Locale> {
    match v.to_ascii_lowercase().as_str() {
        "zh" | "cn" | "zh-cn" | "chinese" => Ok(Locale::Zh),
        "en" | "en-us" | "english" => Ok(Locale::En),
        other => bail!("--lang expects zh or en, got `{other}`"),
    }
}

fn parse_pct(v: &str, flag: &str) -> Result<u8> {
    let n: u16 = v
        .trim_end_matches('%')
        .parse()
        .map_err(|_| anyhow::anyhow!("{flag} expects a percentage, got `{v}`"))?;
    if n > 100 {
        bail!("{flag} must be 0..=100");
    }
    Ok(n as u8)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let Some(args) = parse_args()? else {
        print!("{HELP}");
        return Ok(());
    };

    let offset_secs = local_utc_offset_seconds();
    let locale = args.lang.unwrap_or_else(Locale::detect);

    // Control plane (switch commands + TUI switch pane). Constructing it makes
    // no calls; nothing is written until an explicit `use`/`clear`/apply.
    let control = OmpModelControl::new();
    let switcher = Switcher::new(&control);

    match args.cmd {
        Cmd::Roles => {
            print!("{}", render_roles(&switcher.pins()?, locale));
            return Ok(());
        }
        Cmd::Use => {
            let role = parse_role(args.role.as_deref().unwrap_or(""))?;
            let outcome = switcher.pin(role, args.query.as_deref().unwrap_or(""))?;
            println!("{}", render_outcome(&outcome, locale));
            if matches!(outcome, Outcome::NoMatch { .. } | Outcome::Ambiguous { .. }) {
                return Err(anyhow::anyhow!("no unique model selected"));
            }
            return Ok(());
        }
        Cmd::Clear => {
            let role = parse_role(args.role.as_deref().unwrap_or(""))?;
            let outcome = switcher.clear(role)?;
            println!("{}", render_outcome(&outcome, locale));
            return Ok(());
        }
        Cmd::Models => {
            let models = switcher.models()?;
            print!(
                "{}",
                render_models(
                    &models,
                    args.provider.as_deref(),
                    args.query.as_deref(),
                    locale
                )
            );
            return Ok(());
        }
        Cmd::Show | Cmd::Watch | Cmd::Tui => {}
    }

    let source = OmpUsageSource::new(args.provider.clone());
    let history = FileHistoryStore::new()?;
    let clock = SystemClock;
    let notifier = CliNotifier::new(args.desktop, offset_secs, locale);
    let thresholds = Thresholds {
        warn_pct: args.warn,
        critical_pct: args.critical,
        eta_warn: Duration::from_secs(30 * 60),
    };
    let evaluator = Evaluator::new(&source, &history, thresholds);
    let palette = match args.color {
        Some(false) => Palette::plain(),
        Some(true) => Palette::ansi(),
        None => Palette::auto(),
    };

    if args.cmd == Cmd::Tui {
        headroom::delivery::tui::run(
            &evaluator,
            &switcher,
            Duration::from_secs(args.interval),
            true,
            offset_secs,
            locale,
        )?;
        return Ok(());
    }

    if args.cmd == Cmd::Watch {
        let mut state = WatchState::default();
        loop {
            run_once(
                &evaluator,
                &clock,
                &notifier,
                &palette,
                offset_secs,
                locale,
                Some(&mut state),
            )?;
            std::thread::sleep(Duration::from_secs(args.interval));
        }
    } else {
        run_once(
            &evaluator,
            &clock,
            &notifier,
            &palette,
            offset_secs,
            locale,
            None,
        )?;
    }
    Ok(())
}

/// Parse a role token, mapping failures to an actionable error.
fn parse_role(s: &str) -> Result<Role> {
    Role::parse(s).ok_or_else(|| {
        anyhow::anyhow!("unknown role `{s}` (expected default|plan|slow|smol|advisor)")
    })
}

/// Cross-poll watch state: suppressed alert signatures, plus the critical
/// classes seen last poll (keyed by provider|account|subject) so we can fire a
/// one-shot "refreshed" ping the moment one recovers.
#[derive(Default)]
struct WatchState {
    seen: HashSet<String>,
    crit: HashMap<String, Recovered>,
}

/// One poll + render + notify. In watch mode, suppresses repeat alerts (notifies
/// only newly appeared/escalated) and emits a recovery ping for any critical
/// class that has become available again since the previous poll.
fn run_once(
    evaluator: &Evaluator,
    clock: &SystemClock,
    notifier: &CliNotifier,
    palette: &Palette,
    offset_secs: i32,
    locale: Locale,
    state: Option<&mut WatchState>,
) -> Result<()> {
    use headroom::app::ports::Notifier;

    let assessment = evaluator.poll()?;
    print!(
        "{}",
        render(&assessment, clock.now(), offset_secs, palette, locale)
    );

    let alerts = assessment.alerts();
    match state {
        None => {
            if !alerts.is_empty() {
                notifier.notify(&alerts)?;
            }
        }
        Some(state) => {
            let current: HashSet<String> = alerts.iter().map(signature).collect();
            let fresh: Vec<Alert> = alerts
                .iter()
                .filter(|a| !state.seen.contains(&signature(a)))
                .cloned()
                .collect();
            state.seen = current;

            // Recovery: critical classes present last poll, gone this poll.
            let now_crit: HashMap<String, Recovered> = alerts
                .iter()
                .filter(|a| a.level == AlertLevel::Critical)
                .map(|a| (crit_key(a), recovered_of(a)))
                .collect();
            let prev = std::mem::take(&mut state.crit);
            let recovered = take_recovered(prev, &now_crit);
            state.crit = now_crit;

            if !fresh.is_empty() {
                notifier.notify(&fresh)?;
            }
            if !recovered.is_empty() {
                notifier.notify_recovery(&recovered);
            }
        }
    }
    Ok(())
}

fn crit_key(a: &Alert) -> String {
    format!("{}|{}|{}", a.provider, a.account, a.subject)
}

fn recovered_of(a: &Alert) -> Recovered {
    Recovered {
        provider: a.provider.clone(),
        account: a.account.clone(),
        subject: a.subject.clone(),
    }
}

/// Critical classes present last poll but absent this poll — those that just
/// recovered (window reset or freed). Consumes `prev`.
fn take_recovered(
    prev: HashMap<String, Recovered>,
    now_crit: &HashMap<String, Recovered>,
) -> Vec<Recovered> {
    prev.into_iter()
        .filter(|(k, _)| !now_crit.contains_key(k))
        .map(|(_, r)| r)
        .collect()
}

fn signature(a: &Alert) -> String {
    format!(
        "{}|{}|{}|{}",
        a.provider,
        a.account,
        a.subject,
        a.level.tag()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use headroom::domain::{AccountId, ProviderId};

    fn crit(subject: &str) -> Recovered {
        Recovered {
            provider: ProviderId::new("anthropic"),
            account: AccountId::new("f6*"),
            subject: subject.into(),
        }
    }

    #[test]
    fn recovered_when_a_critical_class_disappears() {
        let mut prev = HashMap::new();
        prev.insert("anthropic|f6*|fable".to_string(), crit("fable"));
        prev.insert("anthropic|f6*|base".to_string(), crit("base"));
        // Only `base` is still critical this poll → `fable` recovered.
        let mut now = HashMap::new();
        now.insert("anthropic|f6*|base".to_string(), crit("base"));

        let recovered = take_recovered(prev, &now);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].subject, "fable");
    }

    #[test]
    fn no_recovery_while_still_critical() {
        let mut prev = HashMap::new();
        prev.insert("anthropic|f6*|fable".to_string(), crit("fable"));
        let mut now = HashMap::new();
        now.insert("anthropic|f6*|fable".to_string(), crit("fable"));
        assert!(take_recovered(prev, &now).is_empty());
    }
}
