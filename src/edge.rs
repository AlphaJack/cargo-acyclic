//! The edge: every program the check runs and every file it reads or writes goes through here. `clippy.toml` bans
//! the direct calls everywhere else.

use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};

/// What `cargo metadata` says of the workspace around the current folder, without its dependencies.
#[expect(clippy::disallowed_methods, reason = "the edge runs cargo")]
pub(crate) fn metadata() -> Result<cargo_metadata::Metadata> {
    cargo_metadata::MetadataCommand::new().no_deps().exec().context("cannot run cargo metadata")
}

/// rust-analyzer's SCIP index of the workspace at `root`, with `settings` as its configuration, kept in `dir`.
#[expect(clippy::disallowed_methods, reason = "the edge runs rust-analyzer and keeps its files")]
pub(crate) fn index(root: &Path, dir: &Path, settings: &str) -> Result<Vec<u8>> {
    fs::create_dir_all(dir)?;
    let (config, index) = (dir.join("config.json"), dir.join("index.scip"));
    fs::write(&config, settings)?;
    let out = Command::new("rust-analyzer")
        .arg("scip")
        .arg(root)
        .arg("--output")
        .arg(&index)
        .arg("--config-path")
        .arg(&config)
        .arg("--exclude-vendored-libraries")
        .output()
        .context("cannot run rust-analyzer (rustup component add rust-analyzer)")?;
    if !out.status.success() {
        bail!("rust-analyzer failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(fs::read(&index)?)
}

/// A source file's text.
#[expect(clippy::disallowed_methods, reason = "the edge reads files")]
pub(crate) fn source(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
}
