//! `gpp-tui` — interactive terminal UI (client interface).
//!
//! [`Dashboard`] is a pure snapshot aggregated from the on-disk stores
//! (timeline / history / trust / anomaly / cost / reviews) — it is unit
//! tested without a TTY. [`run`] is the thin `ratatui` event loop that
//! renders panels and handles keyboard navigation.
//!
//! See `docs/CLI_SPEC.md` (§ gpp ui), `docs/ROADMAP.md` (Phase 8).
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout as RLayout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Timeline,
    History,
    Graphex,
    Agents,
    Reviews,
    Anomalies,
    Cost,
    Inbox,
}

impl Panel {
    pub fn all() -> &'static [Panel] {
        use Panel::*;
        &[
            Timeline, History, Graphex, Agents, Reviews, Anomalies, Cost, Inbox,
        ]
    }
    pub fn title(self) -> &'static str {
        match self {
            Panel::Timeline => "Timeline",
            Panel::History => "History",
            Panel::Graphex => "Graphex",
            Panel::Agents => "Agents",
            Panel::Reviews => "Reviews",
            Panel::Anomalies => "Anomalies",
            Panel::Cost => "Cost",
            Panel::Inbox => "Inbox",
        }
    }
    pub fn parse(s: &str) -> Option<Panel> {
        Some(match s {
            "timeline" => Panel::Timeline,
            "history" => Panel::History,
            "graphex" => Panel::Graphex,
            "agents" => Panel::Agents,
            "reviews" => Panel::Reviews,
            "anomalies" => Panel::Anomalies,
            "cost" => Panel::Cost,
            "inbox" => Panel::Inbox,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutPreset {
    Default,
    Minimal,
    Review,
    Monitoring,
}

impl LayoutPreset {
    pub fn parse(s: &str) -> Option<LayoutPreset> {
        Some(match s {
            "default" => LayoutPreset::Default,
            "minimal" => LayoutPreset::Minimal,
            "review" => LayoutPreset::Review,
            "monitoring" => LayoutPreset::Monitoring,
            _ => return None,
        })
    }
    /// Panels shown (and their order) for this preset.
    pub fn panels(self) -> Vec<Panel> {
        use Panel::*;
        match self {
            LayoutPreset::Default => Panel::all().to_vec(),
            LayoutPreset::Minimal => vec![Timeline, History],
            LayoutPreset::Review => vec![Reviews, History, Anomalies],
            LayoutPreset::Monitoring => vec![Agents, Anomalies, Cost],
        }
    }
}

/// A point-in-time snapshot of the repo's state for the dashboard.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dashboard {
    pub timeline_entries: usize,
    pub unpromoted: usize,
    pub changesets: usize,
    pub head_short: Option<String>,
    pub agents: Vec<(String, f64, String)>, // (id, score, status)
    pub open_anomalies: usize,
    pub cost_usd: f64,
    pub lines_changed: i64,
    pub pending_reviews: usize,
}

impl Dashboard {
    /// Aggregate from the stores under `gpp_dir` (best-effort: a layer that
    /// is absent contributes zero rather than failing the whole dashboard).
    pub fn collect(gpp_dir: &Path) -> Dashboard {
        let mut d = Dashboard::default();
        let root: PathBuf = gpp_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| gpp_dir.to_path_buf());

        if let Ok(tl) = gpp_timeline::Timeline::open(&root, Vec::<String>::new()) {
            if let Ok(es) = tl.entries(&gpp_timeline::EntryFilter {
                limit: Some(u32::MAX),
                ..Default::default()
            }) {
                d.timeline_entries = es.len();
            }
            d.unpromoted = tl
                .unpromoted_in_range(None, None)
                .map(|v| v.len())
                .unwrap_or(0);
        }

        let store = gpp_core::ObjectStore::open(gpp_dir);
        let refs = gpp_history::RefStore::open(gpp_dir);
        if let Ok(Some(tip)) = refs.head_tip() {
            d.head_short = Some(tip.short());
            if let Ok(log) = gpp_history::walk(&store, Some(tip), 1_000_000) {
                d.changesets = log.len();
            }
        }

        if let Ok(ts) = gpp_trust::TrustStore::open(gpp_dir)
            && let Ok(list) = ts.list()
        {
            d.agents = list
                .into_iter()
                .map(|a| (a.agent_id, a.trust_score, a.status.as_str().to_string()))
                .collect();
        }

        if let Ok(an) = gpp_anomaly::AnomalyStore::open(gpp_dir)
            && let Ok(open) = an.list(true, None, None, 100_000)
        {
            d.open_anomalies = open.len();
        }

        if let Ok(cs) = gpp_cost::CostStore::open(gpp_dir)
            && let Ok(s) = cs.summarize(&gpp_cost::CostFilter::default())
        {
            d.cost_usd = s.cost_microdollars as f64 / 1e6;
            d.lines_changed = s.lines_changed;
        }

        if let Ok(rv) = gpp_review::ReviewStore::open(gpp_dir)
            && let Ok(p) = rv.list(Some(gpp_review::ReviewStatus::Pending))
        {
            d.pending_reviews = p.len();
        }

        d
    }

    /// Lines rendered for a given panel.
    pub fn panel_lines(&self, p: Panel) -> Vec<String> {
        match p {
            Panel::Timeline => vec![
                format!("entries:     {}", self.timeline_entries),
                format!("unpromoted:  {}", self.unpromoted),
            ],
            Panel::History => vec![
                format!("changesets:  {}", self.changesets),
                format!(
                    "HEAD:        cs:{}",
                    self.head_short.clone().unwrap_or_else(|| "(none)".into())
                ),
            ],
            Panel::Graphex => vec!["(run `gpp graphex status` for the graph)".into()],
            Panel::Agents => {
                if self.agents.is_empty() {
                    vec!["(no agents tracked)".into()]
                } else {
                    self.agents
                        .iter()
                        .map(|(id, sc, st)| format!("{id:<22} {sc:>6.1} {st}"))
                        .collect()
                }
            }
            Panel::Reviews => vec![format!("pending:     {}", self.pending_reviews)],
            Panel::Anomalies => vec![format!("unresolved:  {}", self.open_anomalies)],
            Panel::Cost => vec![
                format!("cost:        ${:.4}", self.cost_usd),
                format!("lines:       {}", self.lines_changed),
            ],
            Panel::Inbox => vec!["(use `gpp inbox` for notifications)".into()],
        }
    }
}

/// Run the interactive UI. Requires a TTY (the event loop). `panel` is the
/// initially focused panel; `live` enables periodic auto-refresh.
pub fn run(
    gpp_dir: &Path,
    layout: LayoutPreset,
    focus: Option<Panel>,
    live: bool,
) -> std::io::Result<()> {
    let panels = layout.panels();
    let mut selected = focus
        .and_then(|f| panels.iter().position(|p| *p == f))
        .unwrap_or(0);
    let mut dash = Dashboard::collect(gpp_dir);

    let mut terminal = ratatui::init();
    let result = (|| -> std::io::Result<()> {
        loop {
            terminal.draw(|frame| {
                let cols = RLayout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(20), Constraint::Min(20)])
                    .split(frame.area());

                let items: Vec<ListItem> = panels
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let mut st = Style::default();
                        if i == selected {
                            st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                        }
                        ListItem::new(Line::styled(format!(" {} ", p.title()), st))
                    })
                    .collect();
                frame.render_widget(
                    List::new(items).block(Block::default().borders(Borders::ALL).title("gpp")),
                    cols[0],
                );

                let panel = panels[selected];
                let body = dash.panel_lines(panel).join("\n");
                frame.render_widget(
                    Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(
                        format!(" {} — q quit · Tab next · r refresh ", panel.title()),
                    )),
                    cols[1],
                );
            })?;

            let timeout = if live {
                Duration::from_millis(1000)
            } else {
                Duration::from_secs(3600)
            };
            if event::poll(timeout)? {
                if let Event::Key(k) = event::read()? {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            break;
                        }
                        KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => {
                            selected = (selected + 1) % panels.len();
                        }
                        KeyCode::BackTab | KeyCode::Up | KeyCode::Char('k') => {
                            selected = (selected + panels.len() - 1) % panels.len();
                        }
                        KeyCode::Char('r') => dash = Dashboard::collect(gpp_dir),
                        _ => {}
                    }
                }
            } else if live {
                dash = Dashboard::collect(gpp_dir);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_presets_select_panels() {
        assert_eq!(
            LayoutPreset::Minimal.panels(),
            vec![Panel::Timeline, Panel::History]
        );
        assert_eq!(
            LayoutPreset::Monitoring.panels(),
            vec![Panel::Agents, Panel::Anomalies, Panel::Cost]
        );
        assert_eq!(LayoutPreset::Default.panels().len(), 8);
        assert_eq!(Panel::parse("graphex"), Some(Panel::Graphex));
        assert_eq!(LayoutPreset::parse("review"), Some(LayoutPreset::Review));
        assert_eq!(Panel::parse("nope"), None);
    }

    #[test]
    fn dashboard_collects_from_a_real_repo() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(gpp.join("refs")).unwrap();
        gpp_core::ObjectStore::init(&gpp).unwrap();
        std::fs::write(gpp.join("HEAD"), "ref: refs/main\n").unwrap();

        // Empty repo: dashboard is all-zero but does not panic.
        let dash = Dashboard::collect(&gpp);
        assert_eq!(dash.changesets, 0);
        assert_eq!(dash.pending_reviews, 0);
        assert!(dash.head_short.is_none());

        // A pending review shows up.
        let rv = gpp_review::ReviewStore::open(&gpp).unwrap();
        rv.request("cs:abc", "dev@x.io").unwrap();
        let dash = Dashboard::collect(&gpp);
        assert_eq!(dash.pending_reviews, 1);
        // Panel rendering is non-empty and panic-free for every panel.
        for p in Panel::all() {
            assert!(!dash.panel_lines(*p).is_empty());
        }
    }
}
