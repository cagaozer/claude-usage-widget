// SPDX-License-Identifier: GPL-3.0-or-later
//! Embeds the Windows icon and application manifest into the exe.

fn main() {
    println!("cargo:rerun-if-changed=resources.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // The icon and manifest are Windows resources; skip them when building elsewhere.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    embed_resource::compile("resources.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
