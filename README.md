# cargo acyclic

cargo acyclic checks the structure of a Rust workspace. It finds three kinds of problems:

- Cycles between modules. It also finds cycles between items through their signatures and fields.
- Calls up the module tree. No function calls a free function of a parent module. If a parent and its children share a helper then the helper goes in a module of its own.
- Layers out of order. If the workspace declares layers then a module uses only the layers before its own.

It checks the library of each published crate. If a crate has no library then it checks its binary.

## Install

```sh
rustup component add rust-analyzer
cargo binstall cargo-acyclic
```

## Run

```sh
cargo acyclic

# the module graph as a Mermaid flowchart with problems in bold red
cargo acyclic --mermaid > layout.mmd

# only that module and the modules below it
cargo acyclic --mermaid shop::cart
```

The exit code tells the result:

| Code | Result |
|---|---|
| 0 | No problems |
| 1 | One or more problems |
| 2 | No result, for example because the settings do not parse |

## Optional settings

Put the settings in the root `Cargo.toml` of the workspace. A crate without a workspace uses `[package.metadata.acyclic]` instead.

```toml
[workspace.metadata.acyclic]
# Item cycles to allow. Each entry names the exact cycle and why it is there.
allow = [{ cycle = ["shop::Order", "shop::Line"], why = "an order holds lines that name their order" }]
# Modules in order. Each module may use the modules before it and never one after it.
# An entry is a module or a list of modules on the same layer.
layers = ["shop::money", "shop::catalog", ["shop::cart", "shop::checkout"]]
```

The check fails if a setting names a module or a cycle that is no longer there.

## Internals

- rust-analyzer indexes the workspace as [SCIP](https://github.com/sourcegraph/scip) into `target/acyclic`. The index has every feature and no tests.
- syn tells function bodies from signatures. A module uses what its items name in their bodies too. An item uses only what its signatures and fields name.
