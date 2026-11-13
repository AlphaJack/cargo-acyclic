//! `cargo acyclic`: modules and items do not depend on each other in a cycle, no module calls its ancestors' free
//! functions, and modules use only the layers before their own.
//!
//! The edges come from rust-analyzer's [SCIP](https://github.com/sourcegraph/scip) index of each published crate's
//! library, or its binary if it has none, with every feature and without tests. A module uses what its items name,
//! function bodies included; an item uses what its signatures and fields name, so that a type and a function it
//! returns may call each other. A method, field or variant counts as its type. A module may use its ancestors' types,
//! but not call their free functions: a helper that a parent and its children share goes in a module of its own.
//! `[workspace.metadata.acyclic]` allows cycles and declares layers: see [`config`].
//!
//! Needs `rust-analyzer` (`rustup component add rust-analyzer`). `--mermaid [<module>]` prints only the module graph,
//! or the part below `<module>`, as a Mermaid flowchart, the problems in bold red. It exits as the check does.

mod check;
mod config;
mod edge;
mod graph;
mod layers;
mod mermaid;
mod scip;
mod source;
mod symbol;

use std::{env, process::ExitCode};

use anyhow::{Result, bail};

use self::check::Mode;

/// Exits with 0 if the check finds nothing, 1 if it finds something, 2 if it cannot tell, as for settings that do not
/// parse.
fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // As `cargo acyclic`, cargo passes the subcommand's name first.
    if args.first().is_some_and(|a| a == "acyclic") {
        args.remove(0);
    }
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => check::check(&Mode::Check),
        ["--mermaid"] => check::check(&Mode::Mermaid(None)),
        ["--mermaid", module] => check::check(&Mode::Mermaid(Some(module))),
        _ => bail!("usage: cargo acyclic [--mermaid [<module>]]"),
    }
}
