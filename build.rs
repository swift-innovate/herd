//! Concatenates `src/dashboard2/*` source partials into one static HTML file
//! at `$OUT_DIR/dashboard2.html`, embedded via `include_str!` in server.rs.
//!
//! This is not a general build pipeline — no minification, no bundler, no new
//! dependency. It exists so the ~170KB+ dashboard2 artifact can be authored as
//! separate CSS/JS/HTML files instead of one hand-grown blob (the exact
//! pattern the v2 dashboard redesign exists to escape). `cargo build` remains
//! the only command anyone runs; edits to any listed file trigger a rebuild.

use std::fs;
use std::path::Path;

/// (repo-relative path, wrapper) — concatenated in this order.
const PARTS: &[(&str, Wrapper)] = &[
    ("src/dashboard2/shell_head.html", Wrapper::Raw),
    ("src/dashboard2/design-system.css", Wrapper::Style),
    ("src/dashboard2/shell_body.html", Wrapper::Raw),
    ("src/dashboard2/app.js", Wrapper::Script),
    ("src/dashboard2/mark.js", Wrapper::Script),
    ("src/dashboard2/fleet.js", Wrapper::Script),
    ("src/dashboard2/node_detail.js", Wrapper::Script),
    ("src/dashboard2/analytics.js", Wrapper::Script),
    ("src/dashboard2/sessions.js", Wrapper::Script),
    ("src/dashboard2/settings.js", Wrapper::Script),
    ("src/dashboard2/shell_foot.html", Wrapper::Raw),
];

enum Wrapper {
    Raw,
    Style,
    Script,
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest = Path::new(&out_dir).join("dashboard2.html");

    let mut assembled = String::new();
    for (rel_path, wrapper) in PARTS {
        let full_path = Path::new(&manifest_dir).join(rel_path);
        println!("cargo:rerun-if-changed={}", full_path.display());

        // Missing partials are expected mid-build-out (Phase 1 lands screen by
        // screen) — skip rather than fail so `cargo build` keeps working. Any
        // other read failure (bad encoding, permissions) is a real bug and
        // must not be swallowed the same way — panic loudly instead.
        if !full_path.exists() {
            continue;
        }
        let contents = fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("dashboard2 build: failed to read {}: {}", full_path.display(), e));

        match wrapper {
            Wrapper::Raw => assembled.push_str(&contents),
            Wrapper::Style => {
                assembled.push_str("<style>\n");
                assembled.push_str(&contents);
                assembled.push_str("\n</style>\n");
            }
            Wrapper::Script => {
                assembled.push_str("<script>\n");
                assembled.push_str(&contents);
                assembled.push_str("\n</script>\n");
            }
        }
    }

    fs::write(&dest, assembled).expect("failed to write assembled dashboard2.html");
}
