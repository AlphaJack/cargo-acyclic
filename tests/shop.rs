//! `cargo acyclic` on `tests/shop`, a crate with a cycle, uses of later layers, and settings left from what is gone.

use std::process::Command;

#[test]
#[expect(clippy::disallowed_methods, reason = "the test runs the binary")]
fn a_shop_with_every_problem() {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-acyclic"))
        .arg("acyclic")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("shop"))
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "modules that depend on each other:
  shop::cart -> shop::catalog (src/cart/line.rs:2, 1 use)
  shop::catalog -> shop::checkout (src/catalog.rs:2, 1 use)
  shop::checkout -> shop::money (src/checkout.rs:2, 1 use)
  shop::money -> shop::cart (src/money.rs:2, 1 use)

modules that use a later layer:
  shop::catalog -> shop::checkout (src/catalog.rs:2, 1 use)
  shop::money -> shop::cart::line (src/money.rs:2, 1 use)

layers that name no module; fix the acyclic settings:
  shop::basket

allowed cycles that are no longer cycles; remove them from the acyclic settings:
  shop::Order, shop::Line (a cycle that is gone)
"
    );
    assert_eq!(out.status.code(), Some(1));
}
