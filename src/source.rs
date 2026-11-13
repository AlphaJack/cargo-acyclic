//! What SCIP does not say, read from the source with syn: where function bodies are and what `extern` blocks declare.

use anyhow::Result;
use proc_macro2::{LineColumn, extra::DelimSpan};
use syn::visit::{self, Visit};

use super::scip::{Occurrence, Range, contains};

/// Marks what function bodies name and what `extern` blocks declare. rust-analyzer may index two declarations of the
/// same C function as one item at the crate's root, which would look like a call up to it.
pub(crate) fn mark(occurrences: &mut [Occurrence], source: &str) -> Result<()> {
    let mut blocks = Blocks { lines: source.lines().collect(), bodies: Vec::new(), foreign: Vec::new() };
    blocks.visit_file(&syn::parse_file(source)?);
    for o in occurrences {
        o.in_body = blocks.bodies.iter().any(|b| contains(*b, o.range));
        o.foreign = o.definition && blocks.foreign.iter().any(|b| contains(*b, o.range));
    }
    Ok(())
}

/// The braces of function bodies and of `extern` blocks.
struct Blocks<'s> {
    lines: Vec<&'s str>,
    bodies: Vec<Range>,
    foreign: Vec<Range>,
}

impl Blocks<'_> {
    /// From the opening brace to the closing one, as SCIP counts: lines from zero, columns in bytes.
    fn range(&self, braces: &DelimSpan) -> Range {
        let at = |p: LineColumn| {
            let text = self.lines.get(p.line - 1).copied().unwrap_or_default();
            let column = text.char_indices().nth(p.column).map_or(text.len(), |(byte, _)| byte);
            [p.line as u64 - 1, column as u64]
        };
        let ([line, column], [end_line, end]) = (at(braces.open().start()), at(braces.close().end()));
        [line, column, end_line, end]
    }
}

impl<'ast> Visit<'ast> for Blocks<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.bodies.push(self.range(&f.block.brace_token.span));
        visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.bodies.push(self.range(&f.block.brace_token.span));
        visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        if let Some(body) = &f.default {
            self.bodies.push(self.range(&body.brace_token.span));
        }
        visit::visit_trait_item_fn(self, f);
    }

    fn visit_item_foreign_mod(&mut self, block: &'ast syn::ItemForeignMod) {
        self.foreign.push(self.range(&block.brace_token.span));
        visit::visit_item_foreign_mod(self, block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::Symbol;

    /// The occurrence of `word` on `line` of `source`.
    fn at(source: &str, line: usize, word: &str, definition: bool) -> Occurrence {
        let column = source.lines().nth(line).unwrap().find(word).unwrap();
        let (line, column) = (line as u64, column as u64);
        let range = [line, column, line, column + word.len() as u64];
        let symbol = Symbol::parse("item().").unwrap();
        Occurrence { range, symbol, definition, enclosing: None, in_body: false, foreign: false }
    }

    #[test]
    fn a_body_is_inside_the_braces_after_the_signature() {
        let source = "fn first<const N: usize>(bytes: [u8; { N }]) -> impl Fn() -> Order {\n    Order::new\n}";
        let mut occurrences =
            vec![at(source, 0, "first", true), at(source, 0, "Order", false), at(source, 1, "Order", false)];
        mark(&mut occurrences, source).unwrap();
        let in_body: Vec<bool> = occurrences.iter().map(|o| o.in_body).collect();
        assert_eq!(in_body, [false, false, true]);
    }

    #[test]
    fn a_declaration_has_no_body() {
        let source = "trait Cart {\n    fn total(&self) -> Price;\n}\nfn tax() -> Price { Price::ZERO }";
        let mut occurrences = vec![at(source, 1, "Price", false), at(source, 3, "Price::ZERO", false)];
        mark(&mut occurrences, source).unwrap();
        let in_body: Vec<bool> = occurrences.iter().map(|o| o.in_body).collect();
        assert_eq!(in_body, [false, true]);
    }

    #[test]
    fn braces_in_literals_do_not_open_an_extern_block() {
        let source = "unsafe extern \"C\" {\n    // }\n    fn checkout();\n}\nconst OPEN: char = '{';\nfn refund() {}";
        let mut occurrences = vec![at(source, 2, "checkout", true), at(source, 5, "refund", true)];
        mark(&mut occurrences, source).unwrap();
        let foreign: Vec<bool> = occurrences.iter().map(|o| o.foreign).collect();
        assert_eq!(foreign, [true, false]);
    }
}
