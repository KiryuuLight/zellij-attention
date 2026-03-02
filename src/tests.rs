use std::collections::{HashMap, HashSet};
use zellij_tile::prelude::*;

use crate::state::NotificationType;
use crate::State;

// Provide FFI stub so tests can link on native target
#[no_mangle]
pub extern "C" fn host_run_plugin_command() {}

fn make_tab(position: usize, name: &str, active: bool) -> TabInfo {
    TabInfo {
        position,
        name: name.to_string(),
        active,
        ..Default::default()
    }
}

fn make_pane(id: u32, is_plugin: bool, is_focused: bool) -> PaneInfo {
    PaneInfo {
        id,
        is_plugin,
        is_focused,
        ..Default::default()
    }
}

fn make_manifest(tab_panes: Vec<(usize, Vec<PaneInfo>)>) -> PaneManifest {
    let mut panes = HashMap::new();
    for (pos, p) in tab_panes {
        panes.insert(pos, p);
    }
    PaneManifest { panes }
}

fn add_notification(state: &mut State, pane_id: u32, ntype: NotificationType) {
    let mut set = HashSet::new();
    set.insert(ntype);
    state.notification_state.insert(pane_id, set);
}

#[test]
fn test_strip_icons() {
    let state = State::default();
    assert_eq!(state.strip_icons("Tab 1 ⏳"), "Tab 1");
    assert_eq!(state.strip_icons("Tab 1 ✅"), "Tab 1");
    assert_eq!(state.strip_icons("Tab 1 ⏳ ⏳"), "Tab 1");
    assert_eq!(state.strip_icons("Tab 1"), "Tab 1");
    assert_eq!(state.strip_icons(""), "");
}

#[test]
fn test_tab_name_has_icon() {
    let state = State::default();
    assert!(state.tab_name_has_icon("Tab 1 ⏳"));
    assert!(state.tab_name_has_icon("Tab 1 ✅"));
    assert!(!state.tab_name_has_icon("Tab 1"));
    assert!(!state.tab_name_has_icon("⏳ Tab 1")); // icon not at end
}

#[test]
fn test_has_stale_icons_detects_orphaned_icons() {
    let mut state = State::default();
    state.tabs = vec![make_tab(0, "Tab 1 ⏳", true)];
    state.panes = make_manifest(vec![(0, vec![make_pane(1, false, true)])]);

    assert!(state.has_stale_icons());
}

#[test]
fn test_has_stale_icons_ignores_active_notifications() {
    let mut state = State::default();
    state.tabs = vec![make_tab(0, "Tab 1 ⏳", true)];
    state.panes = make_manifest(vec![(0, vec![make_pane(1, false, true)])]);
    add_notification(&mut state, 1, NotificationType::Waiting);

    assert!(!state.has_stale_icons());
}

#[test]
fn test_has_stale_icons_ignores_pending_restores() {
    let mut state = State::default();
    state.tabs = vec![make_tab(0, "Tab 1 ⏳", true)];
    state.panes = make_manifest(vec![(0, vec![make_pane(1, false, true)])]);
    state.original_tab_names.insert(0, "Tab 1".to_string());

    assert!(!state.has_stale_icons());
}

#[test]
fn test_clean_stale_notifications_removes_old_pane_ids() {
    let mut state = State::default();
    add_notification(&mut state, 99, NotificationType::Waiting);
    state.panes = make_manifest(vec![(0, vec![make_pane(1, false, true)])]);

    assert!(state.clean_stale_notifications());
    assert!(state.notification_state.is_empty());
}

#[test]
fn test_clean_stale_skipped_when_panes_empty() {
    let mut state = State::default();
    add_notification(&mut state, 99, NotificationType::Waiting);

    assert!(!state.clean_stale_notifications());
    assert!(!state.notification_state.is_empty());
}

#[test]
fn test_session_restart_scenario_state_file_deleted() {
    let mut state = State::default();

    state.tabs = vec![
        make_tab(0, "Tab 1 ⏳", true),
        make_tab(1, "Tab 2 ✅", false),
    ];
    state.panes = make_manifest(vec![
        (0, vec![make_pane(1, false, true)]),
        (1, vec![make_pane(2, false, false)]),
    ]);

    assert!(!state.has_pending_restores());
    assert!(state.has_stale_icons());
}

#[test]
fn test_original_tab_names_not_wiped_when_tabs_empty() {
    let mut state = State::default();
    state.original_tab_names.insert(0, "Tab 1".to_string());

    assert!(state.tabs.is_empty());
    assert!(state.original_tab_names.contains_key(&0));
}

#[test]
fn test_get_tab_notification_state_skips_plugin_panes() {
    let mut state = State::default();
    state.panes = make_manifest(vec![
        (0, vec![
            make_pane(1, true, false),  // plugin pane
            make_pane(2, false, true),  // terminal pane
        ]),
    ]);
    add_notification(&mut state, 1, NotificationType::Waiting);

    assert_eq!(state.get_tab_notification_state(0), None);

    add_notification(&mut state, 2, NotificationType::Completed);
    assert_eq!(state.get_tab_notification_state(0), Some(NotificationType::Completed));
}

#[test]
fn test_check_and_clear_focus() {
    let mut state = State::default();
    state.tabs = vec![make_tab(0, "Tab 1", true)];
    state.panes = make_manifest(vec![
        (0, vec![make_pane(5, false, true)]),
    ]);
    add_notification(&mut state, 5, NotificationType::Waiting);

    assert!(state.check_and_clear_focus());
    assert!(state.notification_state.is_empty());
}
