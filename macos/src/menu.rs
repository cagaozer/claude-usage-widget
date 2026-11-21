// SPDX-License-Identifier: GPL-3.0-or-later
//! Turns the shared menu model (`usage_core::menu`) into a native `NSMenu`. Every item
//! targets the widget view's `menuItemClicked:` and carries its command id as the tag.

use crate::panel::WidgetView;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::NSString;
use usage_core::menu::{MenuEntry, MenuItem};

/// `NSControlStateValueOn` / `Off`: shows or hides the checkmark.
const STATE_ON: isize = 1;
const STATE_OFF: isize = 0;

fn new_menu(mtm: MainThreadMarker, title: &str) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    // We set `enabled` ourselves; don't let AppKit re-validate items.
    menu.setAutoenablesItems(false);
    menu
}

fn native_item(mtm: MainThreadMarker, it: &MenuItem, target: &WidgetView) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&it.label),
            Some(sel!(menuItemClicked:)),
            &NSString::from_str(""),
        )
    };
    let target: &AnyObject = target;
    unsafe { item.setTarget(Some(target)) };
    item.setTag(it.id as isize);
    item.setState(if it.checked { STATE_ON } else { STATE_OFF });
    item.setEnabled(it.enabled);
    item
}

/// Build the context menu for `entries`, with all actions sent to `target`.
pub fn build(mtm: MainThreadMarker, entries: &[MenuEntry], target: &WidgetView) -> Retained<NSMenu> {
    let menu = new_menu(mtm, "");
    for entry in entries {
        match entry {
            MenuEntry::Item(it) => menu.addItem(&native_item(mtm, it, target)),
            MenuEntry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            MenuEntry::Submenu(title, items) => {
                let sub = new_menu(mtm, title);
                for it in items {
                    sub.addItem(&native_item(mtm, it, target));
                }
                let parent = NSMenuItem::new(mtm);
                parent.setTitle(&NSString::from_str(title));
                parent.setSubmenu(Some(&sub));
                menu.addItem(&parent);
            }
        }
    }
    menu
}
