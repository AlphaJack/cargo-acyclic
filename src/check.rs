//! The check: the workspace from `cargo metadata`, its index from rust-analyzer, and what it finds.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use cargo_metadata::{Metadata, Target, TargetKind};

use crate::{
    config::{Allowed, Config, config},
    edge,
    graph::{Graph, Graphs, cycles, graphs, shortest_cycle},
    layers::{unknown, violations},
    mermaid::{Problems, flowchart},
    scip::{Document, Occurrence, documents},
    source::mark,
    symbol::Symbol,
};

/// rust-analyzer's settings: those of `rust-analyzer.*` in an editor.
const SETTINGS: &str = r#"{"cargo": {"features": "all"}, "cfg": {"setTest": false}}"#;

/// What `cargo acyclic` prints.
pub(crate) enum Mode<'a> {
    /// What it finds.
    Check,
    /// Only the module graph as a Mermaid flowchart, problems in red. With a module, only it and those below it.
    Mermaid(Option<&'a str>),
}

/// A crate the workspace publishes: its library, or its binary if it has none.
struct Crate {
    /// In SCIP's symbols: `cargo-apy`.
    package: String,
    /// The target's, in paths: `cargo_apy`.
    name: String,
    /// The target's folder, from the workspace's: `runtime/src/`.
    dir: String,
    /// The package's other targets' roots, from the workspace's folder: `src/main.rs`.
    others: Vec<String>,
}

impl Crate {
    /// Whether `path` is a source of the target, not of another one next to it.
    fn owns(&self, path: &str) -> bool {
        let source = path.strip_prefix(&self.dir);
        source.is_some_and(|s| !s.starts_with("bin/")) && !self.others.iter().any(|o| o == path)
    }
}

/// What the check finds across the crates.
#[derive(Default)]
struct Found {
    /// Each problem's report.
    failures: Vec<String>,
    modules: Graph,
    uses: Graph,
    problems: Problems,
    /// The allowed cycles that are still cycles, by their place in the settings.
    allowed: BTreeSet<usize>,
}

impl Found {
    /// A crate's cycles, but the allowed ones, and its calls up.
    fn add(&mut self, graphs: Graphs, allow: &[Allowed]) {
        for (what, graph) in [("modules", &graphs.modules), ("items", &graphs.items)] {
            for cycle in cycles(graph) {
                let exact = |(_, a): &(usize, &Allowed)| {
                    let mut members: Vec<&String> = a.cycle.iter().collect();
                    members.sort_unstable();
                    members.iter().copied().eq(cycle.iter())
                };
                if let Some((i, _)) = allow.iter().enumerate().find(exact) {
                    self.allowed.insert(i);
                    continue;
                }
                if what == "modules" {
                    self.problems.cyclic.extend(edges_within(graph, &cycle));
                } else {
                    self.problems.items.extend(cycle.iter().cloned());
                }
                self.failures.push(format!("{what} that depend on each other:\n{}", report(graph, &cycle)));
            }
        }
        if !graphs.upcalls.is_empty() {
            let lines: Vec<String> =
                graphs.upcalls.iter().map(|u| format!("  {} -> {} ({})", u.caller, u.callee, u.at)).collect();
            let lines = lines.join("\n");
            self.failures.push(format!("functions that call their module's ancestors' free functions:\n{lines}"));
        }
        self.problems.upcalls.extend(graphs.upcalls.into_iter().map(|u| u.modules));
        self.modules.extend(graphs.modules);
        self.uses.extend(graphs.uses);
    }

    /// Uses of a later layer, and settings that name what is not there.
    fn settle(&mut self, config: &Config) {
        let later = violations(&self.uses, &config.layers);
        if !later.is_empty() {
            let lines: Vec<String> = later.iter().map(|v| format!("  {} -> {} ({})", v.from, v.to, v.uses)).collect();
            self.failures.push(format!("modules that use a later layer:\n{}", lines.join("\n")));
        }
        let later: Vec<(String, String)> = later.iter().map(|v| (v.from.to_owned(), v.to.to_owned())).collect();
        self.problems.layers.extend(later);
        let gone = unknown(&config.layers, &self.modules);
        if !gone.is_empty() {
            self.failures
                .push(format!("layers that name no module; fix the acyclic settings:\n  {}", gone.join("\n  ")));
        }
        let stale: Vec<String> = config
            .allow
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.allowed.contains(i))
            .map(|(_, a)| format!("  {} ({})", a.cycle.join(", "), a.why))
            .collect();
        if !stale.is_empty() {
            self.failures.push(format!(
                "allowed cycles that are no longer cycles; remove them from the acyclic settings:\n{}",
                stale.join("\n")
            ));
        }
    }
}

/// Prints what `mode` asks for. True if the check finds nothing; an error if it cannot tell.
pub(crate) fn check(mode: &Mode<'_>) -> Result<bool> {
    let metadata = edge::metadata()?;
    let (root, crates, config) = (PathBuf::from(&metadata.workspace_root), crates(&metadata), config(&metadata)?);
    let index = edge::index(&root, &PathBuf::from(&metadata.target_directory).join("acyclic"), SETTINGS)?;
    let mut sources: Vec<Vec<Document<String>>> = crates.iter().map(|_| Vec::new()).collect();
    for document in documents(&index) {
        if let Some(k) = crates.iter().position(|k| k.owns(&document.path)) {
            sources[k].push(document);
        }
    }
    let mut found = Found::default();
    for (krate, documents) in crates.iter().zip(sources) {
        found.add(graphs(&krate.name, &local(&root, krate, documents)?), &config.allow);
    }
    found.settle(&config);
    let clean = found.failures.is_empty();
    match mode {
        // Only the chart, which draws what is wrong, and its report as comments.
        Mode::Mermaid(start) => {
            found.problems.report = found.failures;
            print!("{}", flowchart(&found.modules, &found.problems, *start));
        }
        Mode::Check if clean => {
            let names: Vec<&str> = crates.iter().map(|k| k.name.as_str()).collect();
            let names = names.join(", ");
            println!("in {names}: no cycles between modules or items, no calls up, layers in order");
        }
        Mode::Check => eprintln!("{}", found.failures.join("\n\n")),
    }
    Ok(clean)
}

/// A cycle's shortest loop, each use with where it first happens, and the rest of its knot.
fn report(graph: &Graph, component: &[String]) -> String {
    let cycle = shortest_cycle(graph, component);
    let next = cycle.iter().cycle().skip(1);
    let mut lines: Vec<String> =
        cycle.iter().zip(next).map(|(from, to)| format!("  {from} -> {to} ({})", graph[*from][*to])).collect();
    let rest: Vec<&str> = component.iter().map(String::as_str).filter(|c| !cycle.contains(c)).collect();
    if !rest.is_empty() {
        lines.push(format!("  and in the same knot: {}", rest.join(", ")));
    }
    lines.join("\n")
}

/// The uses between the nodes of a cycle.
fn edges_within(graph: &Graph, component: &[String]) -> Vec<(String, String)> {
    let inside = |node: &String| component.contains(node);
    let edges = component.iter().flat_map(|from| graph[from].keys().filter(|to| inside(to)).map(move |to| (from, to)));
    edges.map(|(from, to)| (from.clone(), to.clone())).collect()
}

/// The workspace's published packages, each with its library, or else its binary named as the package, or else its
/// first binary.
fn crates(metadata: &Metadata) -> Vec<Crate> {
    let relative = |path: &cargo_metadata::camino::Utf8Path| {
        path.strip_prefix(&metadata.workspace_root).map_or_else(|_| path.to_string(), ToString::to_string)
    };
    let library =
        |t: &&Target| t.kind.iter().any(|k| matches!(k, TargetKind::Lib | TargetKind::RLib | TargetKind::ProcMacro));
    let binary = |t: &&Target| t.kind.contains(&TargetKind::Bin);
    // `publish = false` is an empty list of registries.
    let published =
        metadata.workspace_packages().into_iter().filter(|p| p.publish.as_ref().is_none_or(|r| !r.is_empty()));
    let mut crates = Vec::new();
    for package in published {
        let targets = || package.targets.iter();
        let named = || targets().filter(binary).find(|t| t.name == package.name.as_str());
        let Some(target) = targets().find(library).or_else(named).or_else(|| targets().find(binary)) else {
            continue;
        };
        crates.push(Crate {
            package: package.name.to_string(),
            name: target.name.replace('-', "_"),
            dir: target.src_path.parent().map_or_else(String::new, |d| format!("{}/", relative(d))),
            others: targets().filter(|t| t.src_path != target.src_path).map(|t| relative(&t.src_path)).collect(),
        });
    }
    crates
}

/// A crate's files with only its own symbols, parsed, and what the source text tells.
fn local(root: &Path, krate: &Crate, documents: Vec<Document<String>>) -> Result<Vec<Document>> {
    if documents.is_empty() {
        bail!("rust-analyzer indexes no file of {}", krate.package);
    }
    // `rust-analyzer cargo cargo-apy 0.1.0 spec/schema/Schema#`.
    let prefix = format!("rust-analyzer cargo {} ", krate.package);
    let local = |symbol: &str| Some(symbol.strip_prefix(&prefix)?.split_once(' ')?.1.to_owned());
    let mut parsed = Vec::new();
    for Document { path, occurrences } in documents {
        let mut own = Vec::new();
        for o in occurrences {
            let Some(text) = local(&o.symbol) else { continue };
            let symbol = Symbol::parse(&text)
                .with_context(|| format!("{path}: a symbol that SCIP's grammar does not allow: {text}"))?;
            let Occurrence { range, definition, enclosing, .. } = o;
            own.push(Occurrence { range, symbol, definition, enclosing, in_body: false, foreign: false });
        }
        mark(&mut own, &edge::source(&root.join(&path))?).with_context(|| format!("cannot parse {path}"))?;
        parsed.push(Document { path, occurrences: own });
    }
    Ok(parsed)
}
