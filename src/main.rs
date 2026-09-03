//! Composition root: parse args, wire adapters into the use case, render, and
//! (optionally) watch on an interval.

use std::collections::HashSet;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Result};

use headroom::adapters::{CliNotifier, FileHistoryStore, OmpUsageSource, SystemClock};
use headroom::app::ports::Clock;
use headroom::app::Evaluator;
use headroom::delivery::cli::{render, Palette};
use headroom::delivery::Locale;
use headroom::domain::{Alert, Thresholds};

const HELP: &str = "\
headroom — per-model quota headroom for AI coding subscriptions (via omp)

USAGE:
    headroom [OPTIONS]
    headroom watch [OPTIONS]
    headroom tui [OPTIONS]

OPTIONS:
    --provider <id>     Limit to one provider (e.g. anthropic, openai-codex)
    --warn <pct>        Warn threshold, effective remaining % (default 20)
    --critical <pct>    Critical threshold, effective remaining % (default 5)
    --interval <secs>   Poll interval in watch / tui auto-refresh (default 60)
    --no-desktop        Disable macOS desktop notifications
    --no-color          Disable ANSI color
    --lang <zh|en>      Display language (default: auto-detect from locale)
    -h, --help          Print this help

Reads `omp usage --json --redact`; never modifies omp config (read-only).
";

struct Args {
    provider: Option<String>,
    warn: u8,
    critical: u8,
    interval: u64,
    watch: bool,
    tui: bool,
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
            watch: false,
            tui: false,
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
            "watch" => args.watch = true,
            "tui" => args.tui = true,
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

    let source = OmpUsageSource::new(args.provider.clone());
    let history = FileHistoryStore::new()?;
    let clock = SystemClock;
    let locale = args.lang.unwrap_or_else(Locale::detect);
    let notifier = CliNotifier::new(args.desktop, locale);
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

    if args.tui {
        headroom::delivery::tui::run(&evaluator, Duration::from_secs(args.interval), true, locale)?;
        return Ok(());
    }

    if args.watch {
        let mut seen: HashSet<String> = HashSet::new();
        loop {
            run_once(
                &evaluator,
                &clock,
                &notifier,
                &palette,
                locale,
                Some(&mut seen),
            )?;
            std::thread::sleep(Duration::from_secs(args.interval));
        }
    } else {
        run_once(&evaluator, &clock, &notifier, &palette, locale, None)?;
    }
    Ok(())
}

/// One poll + render + notify. In watch mode `seen` suppresses repeat alerts,
/// notifying only newly appeared or escalated ones.
fn run_once(
    evaluator: &Evaluator,
    clock: &SystemClock,
    notifier: &CliNotifier,
    palette: &Palette,
    locale: Locale,
    seen: Option<&mut HashSet<String>>,
) -> Result<()> {
    use headroom::app::ports::Notifier;

    let assessment = evaluator.poll()?;
    print!("{}", render(&assessment, clock.now(), palette, locale));

    let alerts = assessment.alerts();
    let to_notify: Vec<Alert> = match seen {
        None => alerts,
        Some(seen) => {
            let current: HashSet<String> = alerts.iter().map(signature).collect();
            let fresh: Vec<Alert> = alerts
                .into_iter()
                .filter(|a| !seen.contains(&signature(a)))
                .collect();
            *seen = current;
            fresh
        }
    };
    if !to_notify.is_empty() {
        notifier.notify(&to_notify)?;
    }
    Ok(())
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
