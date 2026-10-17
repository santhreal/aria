//! Load external plugin packages from a directory and print their mount status.
//!
//! Run against a plugin directory:
//!
//! ```text
//! cargo run -p aria-plugin --example load_external -- ~/.aria/plugins
//! ```

use std::path::PathBuf;

use aria_plugin::PluginContext;

fn main() {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("usage: load_external <directory>");
            std::process::exit(2);
        });

    let ctx = PluginContext::new();
    let failures = aria_plugin::external::mount_dir(&ctx, &dir);

    println!("{}", dir.display());
    for manifest in aria_plugin::external::loaded() {
        let mounted = ctx.is_mounted(&manifest.id);
        println!(
            "  {} {} ({}) {}",
            if mounted { "mounted" } else { "refused" },
            manifest.id,
            manifest.version,
            manifest.name
        );
    }
    for failure in &failures {
        println!("  failed  {}: {}", failure.id, failure.reason);
    }
    if aria_plugin::external::loaded().is_empty() && failures.is_empty() {
        println!("  no plugin packages found");
    }
}
