//! Interactive full-screen TUI (ratatui + crossterm), localized via `Locale`.
//!
//! Another delivery adapter over the same `Assessment` — domain and app layers
//! untouched. Per-account bar gauges with severity coloring, selection, manual
//! refresh (`r`), periodic auto-refresh, and language toggle (`l`).
//!
//! Data model: quota numbers refresh by polling (`r` or every `interval`); the
//! reset countdown is recomputed every frame. Periodic, not push-live.

use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use super::i18n::Locale;
use super::{
    bar_filled, countdown, display_name, reset_note, severity, truncate, worst_of, Sev, BAR_WIDTH,
};
use crate::app::evaluate::Assessment;
use crate::app::Evaluator;
use crate::domain::{human_duration, AlertLevel};

/// TUI state: the latest assessment, the selected account, and the language.
struct App {
    assessment: Assessment,
    selected: usize,
    updated: SystemTime,
    offset_secs: i32,
    locale: Locale,
}

impl App {
    fn new(assessment: Assessment, offset_secs: i32, locale: Locale) -> Self {
        App {
            assessment,
            selected: 0,
            updated: SystemTime::now(),
            offset_secs,
            locale,
        }
    }

    fn set(&mut self, assessment: Assessment) {
        self.assessment = assessment;
        self.updated = SystemTime::now();
        let n = self.assessment.accounts.len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
    }

    fn next(&mut self) {
        let n = self.assessment.accounts.len();
        if n > 0 {
            self.selected = (self.selected + 1) % n;
        }
    }

    fn prev(&mut self) {
        let n = self.assessment.accounts.len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }
}

fn sev_color(sev: Sev) -> Color {
    match sev {
        Sev::Crit => Color::Red,
        Sev::Warn => Color::Yellow,
        Sev::Ok => Color::Green,
    }
}

fn level_color(level: AlertLevel) -> Color {
    match level {
        AlertLevel::Critical => Color::Red,
        AlertLevel::Warn => Color::Yellow,
        AlertLevel::Info => Color::Gray,
    }
}

/// Enter the alternate screen, run the event loop, and always restore.
pub fn run(
    evaluator: &Evaluator,
    interval: Duration,
    auto: bool,
    offset_secs: i32,
    locale: Locale,
) -> Result<()> {
    let mut app = App::new(evaluator.poll()?, offset_secs, locale);
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, &mut app, evaluator, interval, auto);
    ratatui::restore();
    result
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    evaluator: &Evaluator,
    interval: Duration,
    auto: bool,
) -> Result<()> {
    let mut last = Instant::now();
    loop {
        terminal.draw(|frame| ui(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('r') => {
                            app.set(evaluator.poll()?);
                            last = Instant::now();
                        }
                        KeyCode::Char('l') => app.locale = app.locale.toggle(),
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.prev(),
                        _ => {}
                    }
                }
            }
        }

        if auto && last.elapsed() >= interval {
            app.set(evaluator.poll()?);
            last = Instant::now();
        }
    }
    Ok(())
}

fn ui(frame: &mut Frame, app: &App) {
    let now = SystemTime::now();
    let [body, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

    let secs = now
        .duration_since(app.updated)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let para = Paragraph::new(build_lines(app, now))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(app.locale.tui_title(app.assessment.accounts.len(), secs)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(para, body);

    let hint = Paragraph::new(Line::from(Span::styled(
        app.locale.tui_footer(),
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(hint, footer);
}

fn build_lines(app: &App, now: SystemTime) -> Vec<Line<'static>> {
    let loc = app.locale;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if app.assessment.accounts.is_empty() {
        lines.push(Line::from(loc.no_accounts().to_string()));
        return lines;
    }

    for (i, account) in app.assessment.accounts.iter().enumerate() {
        let snap = &account.snapshot;
        let worst = account
            .classes
            .iter()
            .map(|c| severity(c.headroom.effective_remaining.get(), c.headroom.available))
            .fold(Sev::Ok, worst_of);

        let selected = i == app.selected;
        let mut title_style = Style::default()
            .fg(sev_color(worst))
            .add_modifier(Modifier::BOLD);
        if selected {
            title_style = title_style.add_modifier(Modifier::REVERSED);
        }

        lines.push(Line::from(vec![
            Span::raw(if selected { "▶ " } else { "  " }),
            Span::styled(
                display_name(snap.provider.as_str()).to_string(),
                title_style,
            ),
            Span::styled(
                format!("  {}  ", snap.account),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                snap.plan.as_deref().unwrap_or("-").to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        for class in &account.classes {
            let hr = &class.headroom;
            let rem = hr.effective_remaining.get();
            let color = sev_color(severity(rem, hr.available));
            let filled = bar_filled(rem);
            let resets = hr
                .bottleneck
                .resets_at
                .map(|r| countdown(r, now))
                .unwrap_or_else(|| "—".to_string());
            let eta = class
                .forecast
                .eta
                .map(human_duration)
                .unwrap_or_else(|| "—".to_string());
            let flag = if hr.available {
                String::new()
            } else {
                format!("  ✕ {}", loc.unavailable())
            };

            lines.push(Line::from(vec![
                Span::raw(format!("    {:<7} [", hr.class.label())),
                Span::styled("█".repeat(filled), Style::default().fg(color)),
                Span::styled(
                    "░".repeat(BAR_WIDTH - filled),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("] "),
                Span::styled(
                    format!("{rem:>3}%"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  {:<22} ", truncate(&hr.bottleneck.label, 22))),
                Span::styled(
                    format!("↺{resets}  ~{eta}{flag}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    let alerts = app.assessment.alerts();
    if !alerts.is_empty() {
        lines.push(Line::from(Span::styled(
            loc.alerts_heading().to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for a in &alerts {
            let suggestion = loc
                .alert_suggestion(a)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            let alarm = reset_note(a, now, app.offset_secs, loc)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!(
                    "  {:<4} {}/{} — {}{}{}",
                    loc.level_tag(a.level),
                    display_name(a.provider.as_str()),
                    a.account,
                    loc.alert_reason(a),
                    suggestion,
                    alarm,
                ),
                Style::default().fg(level_color(a.level)),
            )));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::evaluate::{AccountAssessment, ClassAssessment};
    use crate::domain::{
        evaluate, AccountId, ExhaustionForecast, Headroom, LimitScope, LimitWindow, ModelClass,
        Percent, ProviderId, QuotaSnapshot, Thresholds, Tier, WindowStatus,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn account(provider: &str, remaining: u8) -> AccountAssessment {
        let hr = Headroom {
            class: ModelClass::Tier(Tier::new("fable")),
            effective_remaining: Percent::new(remaining),
            bottleneck: LimitWindow {
                id: "7d:fable".into(),
                label: "Claude 7 Day (Fable)".into(),
                scope: LimitScope::Tier(Tier::new("fable")),
                period: None,
                remaining: Percent::new(remaining),
                resets_at: None,
                status: WindowStatus::Ok,
            },
            available: true,
        };
        let alerts = evaluate(
            &ProviderId::new(provider),
            &AccountId::new("f6*"),
            &[(hr.clone(), ExhaustionForecast::default())],
            &Thresholds::default(),
        );
        AccountAssessment {
            snapshot: QuotaSnapshot {
                provider: ProviderId::new(provider),
                account: AccountId::new("f6*"),
                plan: Some("max".into()),
                fetched_at: SystemTime::UNIX_EPOCH,
                windows: vec![],
            },
            classes: vec![ClassAssessment {
                headroom: hr,
                forecast: ExhaustionForecast::default(),
            }],
            alerts,
        }
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let area = buf.area;
        let mut s = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn navigation_wraps_around_accounts() {
        let mut app = App::new(
            Assessment {
                accounts: vec![account("anthropic", 72), account("openai-codex", 50)],
            },
            0,
            Locale::En,
        );
        assert_eq!(app.selected, 0);
        app.next();
        assert_eq!(app.selected, 1);
        app.next();
        assert_eq!(app.selected, 0);
        app.prev();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn renders_chinese_when_locale_zh() {
        let app = App::new(
            Assessment {
                accounts: vec![account("anthropic", 4)],
            },
            0,
            Locale::Zh,
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Claude"), "ascii provider name renders");
        // CJK glyphs are wide and split across TestBackend cells; assert on the
        // content model (build_lines) instead.
        let content: String = build_lines(&app, SystemTime::UNIX_EPOCH)
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("告警"), "localized alerts heading");
        assert!(content.contains("严重"), "localized severity tag");
        assert!(content.contains("仅剩"), "localized alert reason");
    }
}
