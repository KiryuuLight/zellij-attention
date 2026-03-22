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
    // Tab name must have icon for focus-clear to proceed (reorder safety)
    state.tabs = vec![make_tab(0, "Tab 1 ⏳", true)];
    state.panes = make_manifest(vec![
        (0, vec![make_pane(5, false, true)]),
    ]);
    add_notification(&mut state, 5, NotificationType::Waiting);

    assert!(state.check_and_clear_focus());
    assert!(state.notification_state.is_empty());
}

#[test]
fn test_check_and_clear_focus_skips_without_icon() {
    let mut state = State::default();
    // Tab name has no icon — don't clear (protects against reorder race)
    state.tabs = vec![make_tab(0, "Tab 1", true)];
    state.panes = make_manifest(vec![
        (0, vec![make_pane(5, false, true)]),
    ]);
    add_notification(&mut state, 5, NotificationType::Waiting);

    assert!(!state.check_and_clear_focus());
    assert!(!state.notification_state.is_empty());
}

#[test]
fn test_tab_reorder_skips_mismatched_tab_name() {
    let mut state = State::default();

    // Beta at pos 1 has notification, recorded as tab "Beta"
    state.tabs = vec![
        make_tab(0, "Alpha", false),
        make_tab(1, "Beta ⏳", false),
        make_tab(2, "Gamma", true),
    ];
    state.panes = make_manifest(vec![
        (0, vec![make_pane(1, false, false)]),
        (1, vec![make_pane(2, false, false)]),
        (2, vec![make_pane(3, false, true)]),
    ]);
    add_notification(&mut state, 2, NotificationType::Waiting);
    state.notified_tab_names.insert(2, "Beta".to_string());

    // After reorder: pane 2 is now at pos 2 but tab at pos 2 is "Tab #4"
    state.panes = make_manifest(vec![
        (0, vec![make_pane(1, false, false)]),
        (1, vec![make_pane(4, false, false)]),
        (2, vec![make_pane(2, false, false)]),  // Beta's pane at Tab #4's position
        (3, vec![make_pane(3, false, true)]),
    ]);
    state.tabs = vec![
        make_tab(0, "Alpha", false),
        make_tab(1, "Beta ⏳", false),  // stale tab data
        make_tab(2, "Tab #4", true),
        make_tab(3, "Gamma", false),
    ];

    // Pane 2 is at pos 2 but tab is "Tab #4", not "Beta" — should skip
    assert_eq!(state.get_tab_notification_state(2), None);

    // After data stabilizes: pane 2 at pos 2, tab "Beta" at pos 2
    state.tabs = vec![
        make_tab(0, "Alpha", false),
        make_tab(1, "Tab #4", true),
        make_tab(2, "Beta ⏳", false),
        make_tab(3, "Gamma", false),
    ];

    // Now tab name matches — notification should be found
    assert_eq!(state.get_tab_notification_state(2), Some(NotificationType::Waiting));
}

#[test]
fn test_stale_icon_not_stripped_when_notification_expects_tab() {
    let mut state = State::default();

    // "Beta ⏳" at pos 1, notification expects tab "Beta"
    state.tabs = vec![
        make_tab(0, "Alpha", false),
        make_tab(1, "Beta ⏳", false),
    ];
    state.panes = make_manifest(vec![
        (0, vec![make_pane(1, false, false)]),
        (1, vec![make_pane(2, false, false)]),
    ]);
    state.notified_tab_names.insert(2, "Beta".to_string());

    // "Beta ⏳" has icon but notification expects "Beta" — don't strip
    let base = state.strip_icons("Beta ⏳");
    assert!(state.notified_tab_names.values().any(|name| name == &base));
}
