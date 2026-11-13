//! The modules and the items that use each other, and the cycles among them.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
};

use super::{
    scip::{Document, Occurrence, Range, contains, end, start},
    symbol::Symbol,
};

/// A place in the sources: a file from the workspace's folder and a line counted from one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Location {
    pub(crate) file: String,
    pub(crate) line: u64,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

/// How often one node uses another, and the first place it does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Uses {
    pub(crate) count: usize,
    pub(crate) first: Location,
}

impl fmt::Display for Uses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = if self.count == 1 { "" } else { "s" };
        write!(f, "{}, {} use{plural}", self.first, self.count)
    }
}

/// Each node's uses of others.
pub(crate) type Graph = BTreeMap<String, BTreeMap<String, Uses>>;

/// Records a use of `to` by `from` at `at`.
pub(crate) fn add(graph: &mut Graph, from: String, to: String, at: Location) {
    graph
        .entry(from)
        .or_default()
        .entry(to)
        .and_modify(|uses| {
            uses.count += 1;
            uses.first = uses.first.clone().min(at.clone());
        })
        .or_insert(Uses { count: 1, first: at });
}

/// A call from a function up to a free function of its module's ancestor.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Upcall {
    pub(crate) caller: String,
    pub(crate) callee: String,
    /// The caller's module and the ancestor.
    pub(crate) modules: (String, String),
    pub(crate) at: Location,
}

/// What a crate's sources use.
pub(crate) struct Graphs {
    /// Uses between sibling modules.
    pub(crate) modules: Graph,
    /// Uses between any two modules, each named in full: what layers check.
    pub(crate) uses: Graph,
    /// Uses in signatures and fields.
    pub(crate) items: Graph,
    pub(crate) upcalls: BTreeSet<Upcall>,
}

/// The modules and the items of the crate `name` that use each other, and the calls up to an ancestor's free function.
pub(crate) fn graphs(name: &str, documents: &[Document]) -> Graphs {
    // Each definition's item: the outermost definition around it that is not a module, and for a method
    // of an inherent `impl` block, which is no definition, its type. A trait's `impl` is an item of its
    // own, which may be in another module than its type.
    let mut owners = BTreeMap::new();
    for Document { occurrences, .. } in documents {
        for (definition, range) in definitions(occurrences) {
            let outermost = definitions(occurrences)
                .filter(|(d, r)| !d.symbol.is_module() && contains(*r, range))
                .min_by_key(|(_, r)| (start(*r), Reverse(end(*r))));
            if let Some((item, _)) = outermost {
                let owner = self_type(item, occurrences).unwrap_or(&item.symbol);
                owners.insert(&definition.symbol, owner);
            }
        }
    }
    let foreign: BTreeSet<&Symbol> = documents
        .iter()
        .flat_map(|d| &d.occurrences)
        .filter(|o| o.definition && o.foreign)
        .map(|o| &o.symbol)
        .collect();
    let module_path = |m: &[&str]| std::iter::once(name).chain(m.iter().copied()).collect::<Vec<_>>().join("::");
    let mut graphs =
        Graphs { modules: Graph::new(), uses: Graph::new(), items: Graph::new(), upcalls: BTreeSet::new() };
    for Document { path, occurrences } in documents {
        for used in occurrences.iter().filter(|o| !o.definition) {
            // What items name, not `use` declarations: those of test modules are indexed too.
            let within = definitions(occurrences)
                .filter(|(_, r)| contains(*r, used.range))
                .max_by_key(|(_, r)| (start(*r), Reverse(end(*r))));
            let Some((within, _)) = within else { continue };
            let (Some(from), Some(to)) = (owners.get(&within.symbol), owners.get(&used.symbol)) else {
                continue;
            };
            if from == to {
                continue;
            }
            let at = Location { file: path.clone(), line: used.range[0] + 1 };
            // Between sibling modules below their common ancestor: a module and its ancestors may use each
            // other, as a parent re-exports its children and children use the parent's items. By where the
            // code is, which for a trait's `impl` may not be where its type is.
            let (a, b) = (within.symbol.modules(), used.symbol.modules());
            let free = used.symbol.is_free_function() && !foreign.contains(&used.symbol);
            if free && b.len() < a.len() && a.starts_with(&b) {
                graphs.upcalls.insert(Upcall {
                    caller: format!("{name}::{}", within.symbol),
                    callee: format!("{name}::{}", used.symbol),
                    modules: (module_path(&a), module_path(&b)),
                    at: at.clone(),
                });
            }
            if a != b {
                add(&mut graphs.uses, module_path(&a), module_path(&b), at.clone());
            }
            let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
            if common < a.len() && common < b.len() {
                add(&mut graphs.modules, module_path(&a[..=common]), module_path(&b[..=common]), at.clone());
            }
            if !used.in_body {
                add(&mut graphs.items, format!("{name}::{from}"), format!("{name}::{to}"), at);
            }
        }
    }
    // Every module, so that one only its parent uses still shows in the graph.
    for definition in documents.iter().flat_map(|d| &d.occurrences).filter(|o| o.definition) {
        let path = definition.symbol.modules();
        for depth in 1..=path.len() {
            graphs.modules.entry(module_path(&path[..depth])).or_default();
        }
    }
    graphs
}

fn definitions(occurrences: &[Occurrence]) -> impl Iterator<Item = (&Occurrence, Range)> {
    occurrences.iter().filter(|o| o.definition).filter_map(|o| Some((o, o.enclosing?)))
}

/// The self type of a method in an inherent `impl` block: the last type of that name named before the method outside
/// any item, which is the one in the `impl` line. Another type of that name may be named earlier, as in
/// `use crate::shop::Order as ShopOrder;`.
fn self_type<'o>(method: &Occurrence, occurrences: &'o [Occurrence]) -> Option<&'o Symbol> {
    let ty = method.symbol.inherent_self_type()?;
    let items: Vec<Range> = definitions(occurrences).filter(|(d, _)| !d.symbol.is_module()).map(|(_, r)| r).collect();
    occurrences
        .iter()
        .filter(|o| start(o.range) < start(method.range) && !items.iter().any(|r| contains(*r, o.range)))
        .filter(|o| o.symbol.is_type_named(ty))
        .max_by_key(|o| start(o.range))
        .map(|o| &o.symbol)
}

/// The strongly connected components of `graph` with more than one node (Tarjan's algorithm).
pub(crate) fn cycles(graph: &Graph) -> Vec<Vec<String>> {
    struct Search<'g> {
        graph: &'g Graph,
        index: BTreeMap<&'g str, usize>,
        stack: Vec<&'g str>,
        found: Vec<Vec<String>>,
    }
    impl<'g> Search<'g> {
        fn visit(&mut self, node: &'g str) -> usize {
            let order = self.index.len();
            self.index.insert(node, order);
            self.stack.push(node);
            let mut low = order;
            for next in self.graph.get(node).into_iter().flat_map(BTreeMap::keys) {
                low = low.min(match self.index.get(next.as_str()) {
                    None => self.visit(next),
                    Some(&o) if self.stack.contains(&next.as_str()) => o,
                    Some(_) => low,
                });
            }
            if low == order {
                let at = self.stack.iter().rposition(|n| *n == node).unwrap_or_default();
                let mut component: Vec<String> = self.stack.split_off(at).into_iter().map(str::to_owned).collect();
                if component.len() > 1 {
                    component.sort_unstable();
                    self.found.push(component);
                }
            }
            low
        }
    }
    let mut search = Search { graph, index: BTreeMap::new(), stack: Vec::new(), found: Vec::new() };
    for node in graph.keys() {
        if !search.index.contains_key(node.as_str()) {
            search.visit(node);
        }
    }
    search.found
}

/// The shortest cycle among the nodes of `component`, as its nodes in order: the first one breadth-first search finds,
/// so the same graph always gives the same cycle.
pub(crate) fn shortest_cycle<'g>(graph: &'g Graph, component: &[String]) -> Vec<&'g str> {
    let inside = |node: &&String| component.contains(node);
    let mut best: Option<Vec<&str>> = None;
    for first in graph.keys().filter(inside) {
        let (mut parents, mut queue) = (BTreeMap::<&str, &str>::new(), VecDeque::from([first.as_str()]));
        let found = 'search: loop {
            let Some(node) = queue.pop_front() else { break None };
            for next in graph[node].keys().filter(inside) {
                if next == first {
                    break 'search Some(node);
                }
                if !parents.contains_key(next.as_str()) {
                    parents.insert(next, node);
                    queue.push_back(next);
                }
            }
        };
        let Some(last) = found else { continue };
        let mut cycle: Vec<&str> = std::iter::successors(Some(last), |n| parents.get(n).copied()).collect();
        cycle.reverse();
        if best.as_ref().is_none_or(|b| cycle.len() < b.len()) {
            best = Some(cycle);
        }
    }
    best.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &str)]) -> Graph {
        let mut graph = Graph::new();
        for (line, (from, to)) in (1..).zip(edges) {
            add(&mut graph, (*from).into(), (*to).into(), Location { file: "src/lib.rs".into(), line });
        }
        graph
    }

    #[test]
    fn mutual_uses_are_a_cycle() {
        let found = cycles(&graph(&[("a", "b"), ("b", "a"), ("c", "a"), ("b", "d")]));
        assert_eq!(found, [["a", "b"]]);
    }

    #[test]
    fn the_shortest_cycle_of_a_knot() {
        let knot = graph(&[("a", "b"), ("b", "c"), ("c", "d"), ("d", "a"), ("c", "b")]);
        assert_eq!(shortest_cycle(&knot, &cycles(&knot)[0]), ["b", "c"]);
    }

    #[test]
    fn uses_count_and_keep_the_first_place() {
        let uses = graph(&[("a", "b"), ("a", "b")]);
        assert_eq!(uses["a"]["b"], Uses { count: 2, first: Location { file: "src/lib.rs".into(), line: 1 } });
    }

    #[test]
    fn a_method_belongs_to_the_type_of_its_impl_line() {
        let occurrence = |range: Range, symbol: &str, definition: bool, enclosing: Option<Range>| Occurrence {
            range,
            symbol: Symbol::parse(symbol).unwrap(),
            definition,
            enclosing,
            in_body: false,
            foreign: false,
        };
        // use crate::cart::Order as CartOrder;
        // pub struct Order;
        // impl Order {
        //     fn total(&self) {}
        // }
        let occurrences = [
            occurrence([0, 18, 0, 23], "cart/Order#", false, None),
            occurrence([1, 11, 1, 16], "shop/Order#", true, Some([1, 0, 1, 17])),
            occurrence([2, 5, 2, 10], "shop/Order#", false, None),
            occurrence([3, 7, 3, 12], "shop/impl#[Order]total().", true, Some([3, 4, 3, 22])),
        ];
        assert_eq!(self_type(&occurrences[3], &occurrences).map(ToString::to_string).as_deref(), Some("shop::Order"));
    }
}
