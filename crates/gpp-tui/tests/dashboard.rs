//! Integration coverage for [`gpp_tui::Dashboard::collect`] against the real
//! on-disk stores it aggregates. The inline unit tests cover the pure keymap /
//! rendering logic; this exercises the store/db read path end-to-end so each
//! dashboard field is proven to reflect actual repo state, not just defaults.

use gpp_tui::{Dashboard, LayoutPreset, Panel, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// A throwaway repo: `<tmp>/.gpp` initialised like `gpp init`, plus the working
/// root. Returns `(tempdir, gpp_dir, root)`.
fn init_repo() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().to_path_buf();
    let gpp = root.join(".gpp");
    std::fs::create_dir_all(gpp.join("refs")).unwrap();
    gpp_core::ObjectStore::init(&gpp).unwrap();
    std::fs::write(gpp.join("HEAD"), "ref: refs/main\n").unwrap();
    (d, gpp, root)
}

#[test]
fn empty_repo_collects_all_zero_without_panicking() {
    let (_d, gpp, _root) = init_repo();
    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash, Dashboard::default());
    // Every panel renders at least one line even on an empty repo.
    for p in Panel::all() {
        assert!(!dash.panel_lines(*p).is_empty());
    }
}

#[test]
fn timeline_entries_and_unpromoted_are_counted() {
    let (_d, gpp, root) = init_repo();

    let mut tl = gpp_timeline::Timeline::open(&root, Vec::<String>::new()).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    tl.capture(
        gpp_timeline::AuthorKind::Human,
        "dev@x.io",
        gpp_timeline::Source::Cli,
    )
    .unwrap();
    std::fs::write(root.join("b.rs"), "fn b() {}\n").unwrap();
    tl.capture(
        gpp_timeline::AuthorKind::Human,
        "dev@x.io",
        gpp_timeline::Source::Cli,
    )
    .unwrap();

    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash.timeline_entries, 2);
    // Nothing promoted yet, so both entries are unpromoted.
    assert_eq!(dash.unpromoted, 2);
    assert!(dash.panel_lines(Panel::Timeline)[0].contains("entries:     2"));
}

#[test]
fn promotion_populates_history_and_clears_unpromoted() {
    let (_d, gpp, root) = init_repo();

    let mut tl = gpp_timeline::Timeline::open(&root, Vec::<String>::new()).unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    tl.capture(
        gpp_timeline::AuthorKind::Human,
        "dev@x.io",
        gpp_timeline::Source::Cli,
    )
    .unwrap();

    let refs = gpp_history::RefStore::open(&gpp);
    let outcome = gpp_history::promote(
        &mut tl,
        &refs,
        gpp_history::PromoteOptions {
            from: None,
            to: None,
            message: "initial".into(),
            intent_type: gpp_history::IntentType::Feature,
            task: None,
            author: gpp_history::Author::human("Dev", "dev@x.io"),
        },
    )
    .unwrap();
    assert_eq!(outcome.entries_promoted, 1);

    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash.changesets, 1);
    assert_eq!(dash.unpromoted, 0);
    assert!(dash.head_short.is_some());
    assert!(dash.panel_lines(Panel::History)[0].contains("changesets:  1"));
}

#[test]
fn agents_trust_scores_surface() {
    let (_d, gpp, _root) = init_repo();

    let ts = gpp_trust::TrustStore::open(&gpp).unwrap();
    ts.record_event(
        "bot@x.io",
        "Bot",
        Some("claude"),
        "changeset_promoted",
        Some("cs:abc"),
        None,
    )
    .unwrap();

    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash.agents.len(), 1);
    let (id, _score, _status) = &dash.agents[0];
    assert_eq!(id, "bot@x.io");
    assert!(dash.panel_lines(Panel::Agents)[0].contains("bot@x.io"));
}

#[test]
fn open_anomalies_are_counted() {
    let (_d, gpp, _root) = init_repo();

    let an = gpp_anomaly::AnomalyStore::open(&gpp).unwrap();
    // Exceed the default `large-changeset` threshold (800 lines) to raise one.
    let raised = an
        .detect(&gpp_anomaly::ChangesetFacts {
            agent_id: Some("bot@x.io".into()),
            changeset: "cs:big".into(),
            files_touched: 1,
            lines_changed: 5_000,
            recent_changesets_in_window: 0,
        })
        .unwrap();
    assert_eq!(raised.len(), 1);

    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash.open_anomalies, 1);
    assert!(dash.panel_lines(Panel::Anomalies)[0].contains('1'));

    // Resolving it drops the open count back to zero.
    let id = an.list(true, None, None, 10).unwrap()[0].id;
    an.resolve(id, "dev@x.io", "intended large import").unwrap();
    assert_eq!(Dashboard::collect(&gpp).open_anomalies, 0);
}

#[test]
fn cost_records_aggregate() {
    let (_d, gpp, _root) = init_repo();

    let cs = gpp_cost::CostStore::open(&gpp).unwrap();
    cs.record(&gpp_cost::CostRecord {
        changeset_id: "cs:abc".into(),
        agent_id: "bot@x.io".into(),
        model_id: "claude".into(),
        cost_microdollars: 2_500_000, // $2.50
        lines_changed: 120,
        timestamp: 1,
        ..Default::default()
    })
    .unwrap();

    let dash = Dashboard::collect(&gpp);
    assert!((dash.cost_usd - 2.5).abs() < 1e-9);
    assert_eq!(dash.lines_changed, 120);
    assert!(dash.panel_lines(Panel::Cost)[0].contains("$2.5000"));
}

#[test]
fn pending_reviews_are_counted() {
    let (_d, gpp, _root) = init_repo();

    let rv = gpp_review::ReviewStore::open(&gpp).unwrap();
    rv.request("cs:abc", "dev@x.io").unwrap();
    rv.request("cs:def", "dev@x.io").unwrap();

    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash.pending_reviews, 2);
    assert!(dash.panel_lines(Panel::Reviews)[0].contains('2'));
}

#[test]
fn collect_is_resilient_to_a_bare_gpp_dir() {
    // A `.gpp` with nothing in it (no refs/HEAD/dbs) must still collect to a
    // zero dashboard rather than panicking — the "best-effort" contract.
    let d = tempfile::tempdir().unwrap();
    let gpp = d.path().join(".gpp");
    std::fs::create_dir_all(&gpp).unwrap();
    let dash = Dashboard::collect(&gpp);
    assert_eq!(dash.changesets, 0);
    assert_eq!(dash.pending_reviews, 0);
}

/// Flatten a `TestBackend` buffer to a single string for substring assertions.
fn buffer_text(term: &Terminal<TestBackend>) -> String {
    term.backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[test]
fn draw_renders_panel_list_and_focused_body() {
    let dash = Dashboard {
        timeline_entries: 9,
        unpromoted: 2,
        ..Default::default()
    };
    let panels = LayoutPreset::Default.panels();
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    term.draw(|f| draw(f, &panels, 0, &dash)).unwrap();

    let text = buffer_text(&term);
    // Left column shows the app title and panel names; right column shows the
    // focused (Timeline) panel's body and its help hint.
    assert!(text.contains("gpp"));
    assert!(text.contains("Timeline"));
    assert!(text.contains("History"));
    assert!(text.contains("entries:     9"));
    assert!(text.contains("q quit"));
}

#[test]
fn draw_focuses_the_selected_panel() {
    let dash = Dashboard {
        cost_usd: 3.5,
        lines_changed: 99,
        ..Default::default()
    };
    let panels = LayoutPreset::Monitoring.panels(); // [Agents, Anomalies, Cost]
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let cost_idx = panels.iter().position(|p| *p == Panel::Cost).unwrap();
    term.draw(|f| draw(f, &panels, cost_idx, &dash)).unwrap();

    let text = buffer_text(&term);
    // The Cost panel's body (not Agents') is rendered when Cost is selected.
    assert!(text.contains("$3.5000"));
    assert!(text.contains("lines:       99"));
}

#[test]
fn draw_tolerates_out_of_range_selection() {
    // An empty panel set must not panic the renderer (defensive guard).
    let dash = Dashboard::default();
    let mut term = Terminal::new(TestBackend::new(40, 10)).unwrap();
    term.draw(|f| draw(f, &[], 0, &dash)).unwrap();
    // Only the (empty) list block is drawn; the title still appears.
    assert!(buffer_text(&term).contains("gpp"));
}
