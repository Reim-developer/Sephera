//! Tauri's build step, which reads `tauri.conf.json` and generates the context
//! the app embeds. It is the only build script here, and the reason the crate is
//! excluded from the workspace: this runs `tauri-build`, and building it pulls in
//! the webview toolchain that `cargo test --workspace` should not need.

fn main() {
    tauri_build::build();
}
