// SPDX-License-Identifier: GPL-3.0-or-later
//! The right-click menu as plain data, so Windows and macOS show identical menus and
//! interpret clicks identically. Each platform only turns `MenuEntry`s into native menu
//! items and maps the clicked item's id back through `menu_action`.

use crate::controller::Controller;
use crate::settings::{INTERVAL_CHOICES, OPACITY_CHOICES};

// Command ids. Submenu items use base + index (e.g. ID_INTERVAL + 2 = third interval).
pub const ID_REFRESH: usize = 100;
pub const ID_INTERVAL: usize = 110;
pub const ID_OPACITY: usize = 120;
pub const ID_ORG: usize = 200;
pub const ID_SIGN_OUT: usize = 300;
pub const ID_QUIT: usize = 301;
pub const ID_SIGN_IN: usize = 302;

/// What a menu click means.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Refresh,
    Interval(u32),
    Opacity(u8),
    Org(String),
    SignOut,
    SignIn,
    Quit,
}

/// The org uuids listed in the menu, in menu order, captured when the menu was built.
/// A refresh can replace the org list while the menu is open, so a click is decoded
/// against what the user actually saw, not the live list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuSnapshot {
    pub orgs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub id: usize,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuEntry {
    Item(MenuItem),
    Submenu(String, Vec<MenuItem>),
    Separator,
}

/// Decode a clicked command id against the snapshot taken when the menu was shown.
pub fn menu_action(id: usize, snap: &MenuSnapshot) -> Action {
    let in_range = |base: usize, len: usize| (base..base + len).contains(&id);
    match id {
        ID_REFRESH => Action::Refresh,
        ID_SIGN_OUT => Action::SignOut,
        ID_SIGN_IN => Action::SignIn,
        ID_QUIT => Action::Quit,
        _ if in_range(ID_INTERVAL, INTERVAL_CHOICES.len()) => Action::Interval(INTERVAL_CHOICES[id - ID_INTERVAL]),
        _ if in_range(ID_OPACITY, OPACITY_CHOICES.len()) => Action::Opacity(OPACITY_CHOICES[id - ID_OPACITY]),
        _ if in_range(ID_ORG, snap.orgs.len()) => Action::Org(snap.orgs[id - ID_ORG].clone()),
        _ => Action::None,
    }
}

/// Build the menu for the controller's current state:
/// Refresh now · Refresh every ▸ · Opacity ▸ · Organization ▸ (only with > 1 org) ·
/// separator · Sign out / Sign in · Quit.
pub fn build_menu(c: &Controller) -> (Vec<MenuEntry>, MenuSnapshot) {
    let item = |id: usize, label: &str, checked: bool, enabled: bool| MenuItem {
        id,
        label: label.to_string(),
        checked,
        enabled,
    };
    let signed_in = c.is_fetching();
    let mut snap = MenuSnapshot::default();
    let mut entries = vec![MenuEntry::Item(item(ID_REFRESH, "Refresh now", false, signed_in))];

    let intervals = INTERVAL_CHOICES
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let label = if s <= 60 { format!("{s} s") } else { format!("{} min", s / 60) };
            item(ID_INTERVAL + i, &label, c.settings.interval_secs == s, true)
        })
        .collect();
    entries.push(MenuEntry::Submenu("Refresh every".into(), intervals));

    let opacities = OPACITY_CHOICES
        .iter()
        .enumerate()
        .map(|(i, &o)| item(ID_OPACITY + i, &format!("{o}%"), c.settings.opacity_pct == o, true))
        .collect();
    entries.push(MenuEntry::Submenu("Opacity".into(), opacities));

    if c.orgs.len() > 1 {
        let orgs = c
            .orgs
            .iter()
            .enumerate()
            .map(|(i, o)| item(ID_ORG + i, &o.name, c.current_org.as_deref() == Some(&o.uuid), true))
            .collect();
        snap.orgs = c.orgs.iter().map(|o| o.uuid.clone()).collect();
        entries.push(MenuEntry::Submenu("Organization".into(), orgs));
    }

    entries.push(MenuEntry::Separator);
    if signed_in || c.session.is_some() {
        entries.push(MenuEntry::Item(item(ID_SIGN_OUT, "Sign out", false, true)));
    } else {
        entries.push(MenuEntry::Item(item(ID_SIGN_IN, "Sign in", false, true)));
    }
    entries.push(MenuEntry::Item(item(ID_QUIT, "Quit", false, true)));
    (entries, snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::tests::controller;
    use crate::settings::{INTERVAL_CHOICES, OPACITY_CHOICES};
    use crate::usage::Org;

    #[test]
    fn menu_action_uses_snapshot_not_live_state() {
        let snap = MenuSnapshot { orgs: vec!["org-a".into(), "org-b".into()] };
        assert_eq!(menu_action(ID_ORG + 1, &snap), Action::Org("org-b".into()));
        assert_eq!(menu_action(ID_ORG + 2, &snap), Action::None);
        assert_eq!(menu_action(ID_SIGN_OUT, &snap), Action::SignOut);
        assert_eq!(menu_action(ID_SIGN_IN, &snap), Action::SignIn);
        assert_eq!(menu_action(ID_INTERVAL + 2, &snap), Action::Interval(INTERVAL_CHOICES[2]));
        assert_eq!(menu_action(ID_OPACITY + 3, &snap), Action::Opacity(OPACITY_CHOICES[3]));
        assert_eq!(menu_action(ID_REFRESH, &snap), Action::Refresh);
        assert_eq!(menu_action(ID_QUIT, &snap), Action::Quit);
        assert_eq!(menu_action(0, &snap), Action::None);
    }

    fn item(e: &MenuEntry) -> &MenuItem {
        match e {
            MenuEntry::Item(i) => i,
            other => panic!("expected item, got {other:?}"),
        }
    }

    fn submenu(e: &MenuEntry) -> (&str, Vec<(&str, bool)>) {
        match e {
            MenuEntry::Submenu(title, items) => {
                (title.as_str(), items.iter().map(|i| (i.label.as_str(), i.checked)).collect())
            }
            other => panic!("expected submenu, got {other:?}"),
        }
    }

    #[test]
    fn menu_signed_in_two_orgs() {
        let (_dir, mut c) = controller();
        let _ = c.signed_in(crate::session::Session { session_key: "k".into(), last_active_org: None });
        c.orgs = vec![
            Org { uuid: "u1".into(), name: "Personal".into() },
            Org { uuid: "u2".into(), name: "Work".into() },
        ];
        c.current_org = Some("u2".into());

        let (entries, snap) = build_menu(&c);
        assert_eq!(entries.len(), 7);
        let refresh = item(&entries[0]);
        assert_eq!((refresh.label.as_str(), refresh.enabled, refresh.id), ("Refresh now", true, ID_REFRESH));
        assert_eq!(
            submenu(&entries[1]),
            ("Refresh every", vec![("30 s", false), ("60 s", true), ("2 min", false), ("5 min", false)])
        );
        assert_eq!(
            submenu(&entries[2]),
            ("Opacity", vec![("100%", true), ("85%", false), ("70%", false), ("50%", false)])
        );
        assert_eq!(submenu(&entries[3]), ("Organization", vec![("Personal", false), ("Work", true)]));
        assert!(matches!(entries[4], MenuEntry::Separator));
        assert_eq!(item(&entries[5]).label, "Sign out");
        assert_eq!(item(&entries[5]).id, ID_SIGN_OUT);
        assert_eq!(item(&entries[6]).label, "Quit");
        assert_eq!(snap.orgs, vec!["u1".to_string(), "u2".to_string()]);
    }

    #[test]
    fn menu_signed_out_one_org() {
        let (_dir, mut c) = controller();
        c.orgs = vec![Org { uuid: "u1".into(), name: "Personal".into() }];
        let (entries, snap) = build_menu(&c);
        assert_eq!(entries.len(), 6);
        assert!(!item(&entries[0]).enabled);
        assert!(entries.iter().all(|e| !matches!(e, MenuEntry::Submenu(t, _) if t == "Organization")));
        assert_eq!(item(&entries[4]).label, "Sign in");
        assert_eq!(item(&entries[4]).id, ID_SIGN_IN);
        assert!(snap.orgs.is_empty());
    }
}
