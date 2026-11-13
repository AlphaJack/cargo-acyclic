//! Layers: a module on a layer may use the modules of the layers before it, never those of a layer after it.

use crate::{
    config::Layer,
    graph::{Graph, Uses},
};

/// A use of a module on a later layer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Violation<'g> {
    pub(crate) from: &'g str,
    pub(crate) to: &'g str,
    pub(crate) uses: &'g Uses,
}

/// Whether `module` is `listed` or below it.
fn below(module: &str, listed: &str) -> bool {
    module.strip_prefix(listed).is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

/// The layer of `module`: that of the closest listed module it is or is below.
fn layer(module: &str, layers: &[Layer]) -> Option<usize> {
    let listed = layers.iter().enumerate().flat_map(|(i, l)| l.modules().iter().map(move |m| (i, m)));
    listed.filter(|(_, m)| below(module, m)).max_by_key(|(_, m)| m.len()).map(|(i, _)| i)
}

/// Each use, between any two modules, of a module on a later layer.
pub(crate) fn violations<'g>(uses: &'g Graph, layers: &[Layer]) -> Vec<Violation<'g>> {
    let pairs =
        uses.iter().flat_map(|(from, tos)| tos.iter().map(move |(to, uses)| (from.as_str(), to.as_str(), uses)));
    pairs
        .filter(|(from, to, _)| matches!((layer(from, layers), layer(to, layers)), (Some(a), Some(b)) if a < b))
        .map(|(from, to, uses)| Violation { from, to, uses })
        .collect()
}

/// The listed modules that are not in `modules`: settings left from a module that moved or went.
pub(crate) fn unknown<'l>(layers: &'l [Layer], modules: &Graph) -> Vec<&'l str> {
    layers.iter().flat_map(Layer::modules).map(String::as_str).filter(|m| !modules.contains_key(*m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Location, add};

    fn uses(edges: &[(&str, &str)]) -> Graph {
        let mut graph = Graph::new();
        for (line, (from, to)) in (1..).zip(edges) {
            add(&mut graph, (*from).into(), (*to).into(), Location { file: "src/lib.rs".into(), line });
        }
        graph
    }

    #[test]
    fn a_module_may_use_an_earlier_layer_only() {
        let layers =
            [Layer::One("shop::money".into()), Layer::Many(vec!["shop::cart".into(), "shop::checkout".into()])];
        let graph = uses(&[
            ("shop::cart", "shop::money"),
            ("shop::cart", "shop::checkout"),
            ("shop::money::round", "shop::cart::line"),
            ("shop::money", "shop::log"),
        ]);
        let found: Vec<(&str, &str)> = violations(&graph, &layers).iter().map(|v| (v.from, v.to)).collect();
        assert_eq!(found, [("shop::money::round", "shop::cart::line")]);
    }

    #[test]
    fn the_closest_listed_module_decides() {
        let layers = [Layer::One("shop::emit::text".into()), Layer::One("shop::emit".into())];
        assert_eq!(layer("shop::emit::text::escape", &layers), Some(0));
        assert_eq!(layer("shop::emit::rust", &layers), Some(1));
        assert_eq!(layer("shop::emitter", &layers), None);
    }

    #[test]
    fn a_listed_module_that_is_gone_is_reported() {
        let mut graph = uses(&[("shop::cart", "shop::money")]);
        graph.entry("shop::money".into()).or_default();
        let layers = [Layer::One("shop::money".into()), Layer::One("shop::basket".into())];
        assert_eq!(unknown(&layers, &graph), ["shop::basket"]);
    }
}
