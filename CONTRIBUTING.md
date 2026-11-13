# Contributing

## Setup

```sh
rustup component add rust-analyzer   # the check indexes the code with it
cargo install cargo-nextest cargo-deny
```

## Layout

- `src/main.rs`: the arguments and the exit code.
- `src/check.rs`: the check, from the workspace to what it finds.
- `src/edge.rs`: every program run and every file read or written.
- `src/scip.rs`, `src/symbol.rs`, `src/source.rs`: the index, its symbols, and what syn adds.
- `src/graph.rs`, `src/layers.rs`, `src/mermaid.rs`: the graphs, the layers, and the chart.
- `tests/shop`: a crate with every problem, which `tests/shop.rs` checks end to end.

## Rules

- Decision code does not touch files, processes or the environment. They go through `edge`. `clippy.toml` bans the direct calls everywhere else.
- The report does not depend on hash order: `clippy.toml` bans `HashMap` and `HashSet`.
- Unused code goes, and so does code that only an earlier version needed.
- Comments are rustdoc, or say why. None restate the code.
- Code, comments, docs and messages are plain ASCII.
- Examples use one domain: a shop with products, orders and customers.

## Checks

Run these before pushing:

```sh
cargo fmt --all
CARGO_BUILD_WARNINGS=deny cargo clippy --all-targets
cargo nextest run
cargo deny check
```
