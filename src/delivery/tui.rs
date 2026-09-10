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
    bar_filled, countdown, display_name, provider_severity, reset_note, severity, truncate,
    worst_of, Sev, BAR_WIDTH,
};
use crate::app::evaluate::Assessment;
use crate::app::{Evaluator, Outcome, Switcher};
use crate::domain::{human_duration, AlertLevel, ModelRef, Role, RolePins};

/// Which pane is showing: the read-only gauges, or the switch (policy-group)
/// pane where the user manually pins models to roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Gauges,
    Switch,
}

/// TUI state: the latest assessment, the selected account, the language, and —
/// once the switch pane is opened — the model catalog, current pins, and the
/// selector cursor.
struct App {
    assessment: Assessment,
    selected: usize,
    updated: SystemTime,
    offset_secs: i32,
    locale: Locale,
    mode: Mode,
    // Switch-pane state (populated lazily on first entry).
    models: Vec<ModelRef>,
    pins: RolePins,
    role_idx: usize,
    node_idx: usize,
    filter: String,
    filtering: bool,
    status: Option<String>,
    loaded: bool,
}

impl App {
    fn new(assessment: Assessment, offset_secs: i32, locale: Locale) -> Self {
        App {
            assessment,
            selected: 0,
            updated: SystemTime::now(),
            offset_secs,
            locale,
            mode: Mode::Gauges,
            models: Vec::new(),
            pins: RolePins::default(),
            role_idx: 0,
            node_idx: 0,
            filter: String::new(),
            filtering: false,
            status: None,
            loaded: false,
        }
    }

    fn role(&self) -> Role {
        Role::ALL[self.role_idx.min(Role::ALL.len() - 1)]
    }

    /// Models matching the current filter (case-insensitive over selector+name).
    fn filtered(&self) -> Vec<&ModelRef> {
        if self.filter.is_empty() {
            return self.models.iter().collect();
        }
        let q = self.filter.to_ascii_lowercase();
        self.models
            .iter()
            .filter(|m| {
                m.selector.to_ascii_lowercase().contains(&q)
                    || m.name.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    /// Total selectable rows in the node pane: the Auto row plus filtered models.
    fn node_count(&self) -> usize {
        self.filtered().len() + 1
    }

    /// Enter the switch pane, loading the catalog and pins once via `sw`.
    fn enter_switch(&mut self, sw: &Switcher) -> Result<()> {
        if !self.loaded {
            self.models = sw.models()?;
            self.pins = sw.pins()?;
            self.loaded = true;
        }
        self.mode = Mode::Switch;
        self.status = None;
        self.filtering = false;
        self.clamp_node();
        Ok(())
    }

    fn clamp_node(&mut self) {
        let n = self.node_count();
        if self.node_idx >= n {
            self.node_idx = n.saturating_sub(1);
        }
    }

    fn role_next(&mut self) {
        self.role_idx = (self.role_idx + 1) % Role::ALL.len();
        self.node_idx = 0;
    }

    fn role_prev(&mut self) {
        self.role_idx = (self.role_idx + Role::ALL.len() - 1) % Role::ALL.len();
        self.node_idx = 0;
    }

    fn node_next(&mut self) {
        let n = self.node_count();
        if n > 0 {
            self.node_idx = (self.node_idx + 1) % n;
        }
    }

    fn node_prev(&mut self) {
        let n = self.node_count();
        if n > 0 {
            self.node_idx = (self.node_idx + n - 1) % n;
        }
    }

    /// Apply the highlighted node to the current role: row 0 clears the pin
    /// ("Auto"), any other row pins that model. Writes through `sw` and
    /// refreshes the verified pins.
    fn apply(&mut self, sw: &Switcher) -> Result<()> {
        let role = self.role();
        if self.node_idx == 0 {
            let outcome = sw.clear(role)?;
            if let Outcome::Cleared { pins, .. } = outcome {
                self.pins = pins;
            }
            self.status = Some(self.locale.cleared(role));
            return Ok(());
        }
        let selector = {
            let filtered = self.filtered();
            match filtered.get(self.node_idx - 1) {
                Some(m) => m.selector.clone(),
                None => return Ok(()),
            }
        };
        self.pins = sw.pin_selector(role, &selector)?;
        self.status = Some(self.locale.switched(role, &selector));
        Ok(())
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
    switcher: &Switcher,
    interval: Duration,
    auto: bool,
    offset_secs: i32,
    locale: Locale,
) -> Result<()> {
    let mut app = App::new(evaluator.poll()?, offset_secs, locale);
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, &mut app, evaluator, switcher, interval, auto);
    ratatui::restore();
    result
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    evaluator: &Evaluator,
    switcher: &Switcher,
    interval: Duration,
    auto: bool,
) -> Result<()> {
    let mut last = Instant::now();
    loop {
        terminal.draw(|frame| ui(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press
                    && handle_key(app, evaluator, switcher, key.code)?
                {
                    break;
                }
                last = Instant::now();
            }
        }

        if auto && last.elapsed() >= interval {
            app.set(evaluator.poll()?);
            last = Instant::now();
        }
    }
    Ok(())
}

/// Handle one key press. Returns `Ok(true)` to quit the loop.
fn handle_key(
    app: &mut App,
    evaluator: &Evaluator,
    switcher: &Switcher,
    code: KeyCode,
) -> Result<bool> {
    match app.mode {
        Mode::Gauges => match code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
            KeyCode::Char('r') => app.set(evaluator.poll()?),
            KeyCode::Char('l') => app.locale = app.locale.toggle(),
            KeyCode::Char('s') => app.enter_switch(switcher)?,
            KeyCode::Down | KeyCode::Char('j') => app.next(),
            KeyCode::Up | KeyCode::Char('k') => app.prev(),
            _ => {}
        },
        Mode::Switch if app.filtering => match code {
            KeyCode::Enter | KeyCode::Esc => {
                if code == KeyCode::Esc {
                    app.filter.clear();
                }
                app.filtering = false;
                app.node_idx = 0;
            }
            KeyCode::Backspace => {
                app.filter.pop();
                app.node_idx = 0;
            }
            KeyCode::Char(c) => {
                app.filter.push(c);
                app.node_idx = 0;
            }
            _ => {}
        },
        Mode::Switch => match code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('g') | KeyCode::Esc => app.mode = Mode::Gauges,
            KeyCode::Char('l') => app.locale = app.locale.toggle(),
            KeyCode::Char('r') => app.set(evaluator.poll()?),
            KeyCode::Char('/') => app.filtering = true,
            KeyCode::Char('c') => {
                app.node_idx = 0;
                app.apply(switcher)?;
            }
            KeyCode::Enter => app.apply(switcher)?,
            KeyCode::Down | KeyCode::Char('j') => app.node_next(),
            KeyCode::Up | KeyCode::Char('k') => app.node_prev(),
            KeyCode::Left | KeyCode::BackTab => app.role_prev(),
            KeyCode::Right | KeyCode::Tab => app.role_next(),
            _ => {}
        },
    }
    Ok(false)
}

fn ui(frame: &mut Frame, app: &App) {
    match app.mode {
        Mode::Gauges => ui_gauges(frame, app),
        Mode::Switch => ui_switch(frame, app),
    }
}

fn ui_gauges(frame: &mut Frame, app: &App) {
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

/// The Clash-style policy-group pane: roles on the left, candidate models on
/// the right, with the current pin marked and each node health-colored from the
/// provider's live quota.
fn ui_switch(frame: &mut Frame, app: &App) {
    let loc = app.locale;
    let [body, status, footer] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Length(24), Constraint::Min(1)]).areas(body);

    let roles = Paragraph::new(build_role_lines(app)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", loc.groups_heading())),
    );
    frame.render_widget(roles, left);

    let title = if app.filter.is_empty() {
        format!(" {} ", loc.nodes_heading())
    } else {
        format!(" {} · /{}", loc.nodes_heading(), app.filter)
    };
    let node_lines = build_node_lines(app);
    let visible = right.height.saturating_sub(2) as usize; // minus borders
    let scroll = app
        .node_idx
        .saturating_sub(visible / 2)
        .min(node_lines.len().saturating_sub(visible.max(1))) as u16;
    let nodes = Paragraph::new(node_lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((scroll, 0));
    frame.render_widget(nodes, right);

    let role = app.role();
    let status_text = app.status.clone().unwrap_or_else(|| {
        let pin = app.pins.get(role).unwrap_or(loc.unset());
        format!("{} → {}", loc.role_label(role), pin)
    });
    let status_line = Paragraph::new(Line::from(Span::styled(
        format!(" {status_text}"),
        Style::default().fg(Color::Cyan),
    )));
    frame.render_widget(status_line, status);

    let hint = Paragraph::new(Line::from(Span::styled(
        loc.switch_footer(),
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(hint, footer);
}

fn build_role_lines(app: &App) -> Vec<Line<'static>> {
    let loc = app.locale;
    let mut lines = Vec::new();
    for (i, role) in Role::ALL.iter().enumerate() {
        let selected = i == app.role_idx;
        let pinned = app.pins.get(*role).is_some();
        let mut style = Style::default().add_modifier(Modifier::BOLD);
        if selected {
            style = style.add_modifier(Modifier::REVERSED);
        }
        if pinned {
            style = style.fg(Color::Cyan);
        }
        let mark = if selected { "▶ " } else { "  " };
        let tag = if pinned { "●" } else { "○" };
        lines.push(Line::from(vec![
            Span::raw(mark),
            Span::styled(format!("{tag} {}", loc.role_label(*role)), style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("     {}", loc.role_hint(*role)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}

fn build_node_lines(app: &App) -> Vec<Line<'static>> {
    let loc = app.locale;
    let role = app.role();
    let current = app.pins.get(role);
    let mut lines = Vec::new();

    // Row 0: the "Auto" pseudo-node (clear the pin).
    let auto_selected = app.node_idx == 0;
    let auto_current = current.is_none();
    lines.push(node_line(
        loc.auto_node(),
        "",
        auto_selected,
        auto_current,
        None,
        true,
    ));

    for (j, m) in app.filtered().iter().enumerate() {
        let selected = app.node_idx == j + 1;
        let is_current = current == Some(m.selector.as_str());
        let sev = provider_severity(&app.assessment, m.provider.as_str());
        lines.push(node_line(
            &m.selector,
            &m.name,
            selected,
            is_current,
            sev,
            false,
        ));
    }
    lines
}

fn node_line(
    text: &str,
    name: &str,
    selected: bool,
    current: bool,
    sev: Option<Sev>,
    is_auto: bool,
) -> Line<'static> {
    let mark = if selected { "▶ " } else { "  " };
    // Current-pin marker (distinct from the health dot to avoid ●● collisions).
    let cur = if current { "✓ " } else { "  " };
    // Health dot: colored for real nodes with live quota; blank for the Auto row.
    let dot: Span<'static> = if is_auto {
        Span::raw("  ")
    } else {
        let color = sev.map(sev_color).unwrap_or(Color::DarkGray);
        Span::styled("● ", Style::default().fg(color))
    };
    let mut sel_style = Style::default();
    if current {
        sel_style = sel_style.fg(Color::Cyan).add_modifier(Modifier::BOLD);
    }
    if selected {
        sel_style = sel_style.add_modifier(Modifier::REVERSED);
    }
    let mut spans = vec![
        Span::raw(mark),
        Span::styled(cur, Style::default().fg(Color::Cyan)),
        dot,
        Span::styled(truncate(text, 52), sel_style),
    ];
    if !name.is_empty() && name != text {
        spans.push(Span::styled(
            format!("  {}", truncate(name, 28)),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
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

    // A fake ModelControl backing the switch pane in tests — an in-memory
    // catalog and pins, with the same read-back-verify contract.
    struct FakeControl {
        models: Vec<ModelRef>,
        pins: std::cell::RefCell<RolePins>,
    }

    impl FakeControl {
        fn new() -> Self {
            let models = vec![
                ModelRef::new(
                    ProviderId::new("anthropic"),
                    "anthropic/claude-opus-4",
                    "Claude Opus 4",
                ),
                ModelRef::new(
                    ProviderId::new("anthropic"),
                    "anthropic/claude-haiku-4",
                    "Claude Haiku 4",
                ),
                ModelRef::new(
                    ProviderId::new("openai-codex"),
                    "openai-codex/gpt-5.3-codex",
                    "GPT-5.3 Codex",
                ),
            ];
            FakeControl {
                models,
                pins: std::cell::RefCell::new(RolePins::default()),
            }
        }
    }

    impl crate::app::ModelControl for FakeControl {
        fn available_models(&self) -> anyhow::Result<Vec<ModelRef>> {
            Ok(self.models.clone())
        }
        fn pins(&self) -> anyhow::Result<RolePins> {
            Ok(self.pins.borrow().clone())
        }
        fn write_pins(&self, pins: &RolePins) -> anyhow::Result<RolePins> {
            *self.pins.borrow_mut() = pins.clone();
            Ok(pins.clone())
        }
    }

    fn switch_app() -> (App, Assessment) {
        let assessment = Assessment {
            accounts: vec![account("anthropic", 72)],
        };
        (App::new(assessment.clone(), 0, Locale::En), assessment)
    }

    #[test]
    fn switch_pane_lists_roles_auto_and_models() {
        let control = FakeControl::new();
        let switcher = Switcher::new(&control);
        let (mut app, _) = switch_app();
        app.enter_switch(&switcher).unwrap();
        assert_eq!(app.mode, Mode::Switch);

        let roles: String = build_role_lines(&app)
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(roles.contains("Default"));
        assert!(roles.contains("Advisor"));

        let nodes: String = build_node_lines(&app)
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        // Auto pseudo-node first, then the catalog.
        assert!(nodes.contains("Auto"));
        assert!(nodes.contains("anthropic/claude-opus-4"));
        assert!(nodes.contains("openai-codex/gpt-5.3-codex"));
        // node_count = auto + 3 models.
        assert_eq!(app.node_count(), 4);
    }

    #[test]
    fn applying_a_node_pins_the_role_and_clearing_reverts() {
        let control = FakeControl::new();
        let switcher = Switcher::new(&control);
        let (mut app, _) = switch_app();
        app.enter_switch(&switcher).unwrap();

        // Select the first real model (row 1) for the Default role and apply.
        app.node_next();
        assert_eq!(app.node_idx, 1);
        app.apply(&switcher).unwrap();
        assert_eq!(app.role(), Role::Default);
        assert_eq!(app.pins.get(Role::Default), Some("anthropic/claude-opus-4"));
        assert_eq!(
            control.pins.borrow().get(Role::Default),
            Some("anthropic/claude-opus-4"),
            "write reached the control"
        );
        assert!(app.status.as_deref().unwrap().contains("pinned"));

        // Clearing (row 0 = Auto) removes the pin.
        app.node_idx = 0;
        app.apply(&switcher).unwrap();
        assert_eq!(app.pins.get(Role::Default), None);
        assert!(control.pins.borrow().get(Role::Default).is_none());
    }

    #[test]
    fn filter_narrows_nodes_and_role_switch_resets_cursor() {
        let control = FakeControl::new();
        let switcher = Switcher::new(&control);
        let (mut app, _) = switch_app();
        app.enter_switch(&switcher).unwrap();

        app.filter = "codex".to_string();
        // auto + 1 codex model.
        assert_eq!(app.node_count(), 2);
        app.node_next();
        assert_eq!(app.node_idx, 1);
        app.role_next();
        assert_eq!(app.node_idx, 0, "changing role resets the node cursor");
    }

    #[test]
    fn switch_pane_renders_without_panic() {
        let control = FakeControl::new();
        let switcher = Switcher::new(&control);
        let (mut app, _) = switch_app();
        app.enter_switch(&switcher).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("anthropic/claude-opus-4"));
    }
}
