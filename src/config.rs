//! What a workspace asks of `cargo acyclic`: `[workspace.metadata.acyclic]` in its root `Cargo.toml`, or
//! `[package.metadata.acyclic]` in a package without a workspace.
//!
//! ```toml
//! [workspace.metadata.acyclic]
//! # Item cycles allowed, each exactly, with why. Each must still be a cycle.
//! allow = [{ cycle = ["shop::Order", "shop::Line"], why = "an order holds lines, which name their order" }]
//! # Modules in order: each may use those before it, never one after it. An entry is a module, or a list of modules
//! # on the same layer. A module below a listed one is on its layer; one not listed is on none.
//! layers = ["shop::money", "shop::catalog", ["shop::cart", "shop::checkout"]]
//! ```

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) allow: Vec<Allowed>,
    pub(crate) layers: Vec<Layer>,
}

/// A cycle of items that may stay, such as a recursive data type a specification defines.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Allowed {
    /// Its items, in any order: `shop::Order`.
    pub(crate) cycle: Vec<String>,
    pub(crate) why: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum Layer {
    One(String),
    Many(Vec<String>),
}

impl Layer {
    pub(crate) fn modules(&self) -> &[String] {
        match self {
            Self::One(module) => std::slice::from_ref(module),
            Self::Many(modules) => modules,
        }
    }
}

/// The workspace's settings, or none.
pub(crate) fn config(metadata: &cargo_metadata::Metadata) -> Result<Config> {
    let package = || metadata.root_package().and_then(|p| p.metadata.get("acyclic"));
    let Some(table) = metadata.workspace_metadata.get("acyclic").or_else(package) else {
        return Ok(Config::default());
    };
    Config::deserialize(table).context("in [workspace.metadata.acyclic]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_is_a_module_or_a_list() {
        let table = serde_json::json!({
            "allow": [{ "cycle": ["shop::Order", "shop::Line"], "why": "lines name their order" }],
            "layers": ["shop::money", ["shop::cart", "shop::checkout"]],
        });
        let config = Config::deserialize(&table).unwrap();
        assert_eq!(config.allow[0].cycle, ["shop::Order", "shop::Line"]);
        let layers: Vec<&[String]> = config.layers.iter().map(Layer::modules).collect();
        assert_eq!(layers, [&["shop::money".to_owned()][..], &["shop::cart".to_owned(), "shop::checkout".to_owned()]]);
    }

    #[test]
    fn an_unknown_setting_fails() {
        assert!(Config::deserialize(&serde_json::json!({ "layer": [] })).is_err());
    }
}
