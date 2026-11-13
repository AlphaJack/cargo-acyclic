//! The module graph as a Mermaid flowchart.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use super::graph::Graph;

/// What the check finds, which the flowchart draws in red.
#[derive(Default)]
pub(crate) struct Problems {
    /// Uses between modules in a cycle.
    pub(crate) cyclic: BTreeSet<(String, String)>,
    /// Calls up to an ancestor's free function, by module.
    pub(crate) upcalls: BTreeSet<(String, String)>,
    /// Uses of a module on a later layer.
    pub(crate) layers: BTreeSet<(String, String)>,
    /// Items in a cycle, each drawn as the closest module around it.
    pub(crate) items: BTreeSet<String>,
    /// The check's report, as comments at the top of the chart.
    pub(crate) report: Vec<String>,
}

/// Mermaid's style for what is wrong: red and bold.
const RED: &str = "stroke:red,stroke-width:3px";

/// A flowchart, which `Display` writes as Mermaid: comments, then nodes, then links in columns, then what is red.
struct Chart(Vec<Statement>);

enum Statement {
    /// `%% text`, one per line.
    Comment(String),
    /// A module without others below it.
    Node {
        id: String,
        label: String,
        red: bool,
    },
    /// A box around the modules below a module.
    Subgraph {
        id: String,
        label: String,
        red: bool,
        inside: Vec<Self>,
    },
    Link {
        from: String,
        arrow: Arrow,
        to: String,
    },
}

#[derive(Clone, Copy)]
enum Arrow {
    /// A use.
    Plain,
    /// Two modules that use each other.
    Both,
    /// A use in a cycle.
    Bold,
    /// A call up to an ancestor's free function.
    CallUp,
    /// A use of a module on a later layer.
    Layer,
}

impl Arrow {
    const fn text(self) -> &'static str {
        match self {
            Self::Plain => "-->",
            Self::Both => "<==>",
            Self::Bold => "==>",
            Self::CallUp => "-.->|calls up|",
            Self::Layer => "==>|later layer|",
        }
    }
}

impl fmt::Display for Chart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "flowchart LR")?;
        for statement in &self.0 {
            if let Statement::Comment(text) = statement {
                for line in text.lines() {
                    writeln!(f, "    %% {line}")?;
                }
            }
        }
        let mut red_nodes = Vec::new();
        nodes(f, &self.0, 1, &mut red_nodes)?;
        let links: Vec<(&str, Arrow, &str)> = self
            .0
            .iter()
            .filter_map(|s| {
                if let Statement::Link { from, arrow, to } = s {
                    Some((from.as_str(), *arrow, to.as_str()))
                } else {
                    None
                }
            })
            .collect();
        // In columns, which Mermaid ignores and a reader of the source does not.
        let from_width = links.iter().map(|(from, ..)| from.len()).max().unwrap_or_default();
        let arrow_width = links.iter().map(|(_, arrow, _)| arrow.text().len()).max().unwrap_or_default();
        for (from, arrow, to) in &links {
            writeln!(f, "    {from:from_width$} {:arrow_width$} {to}", arrow.text())?;
        }
        let red: Vec<String> = links
            .iter()
            .enumerate()
            .filter(|(_, (_, arrow, _))| !matches!(arrow, Arrow::Plain))
            .map(|(i, _)| i.to_string())
            .collect();
        if !red.is_empty() {
            writeln!(f, "    linkStyle {} {RED}", red.join(","))?;
        }
        for id in red_nodes {
            writeln!(f, "    style {id} {RED}")?;
        }
        Ok(())
    }
}

/// The nodes and boxes among `statements`, indented by `depth`. Collects the red ones' ids.
fn nodes<'s>(
    f: &mut fmt::Formatter<'_>,
    statements: &'s [Statement],
    depth: usize,
    red_nodes: &mut Vec<&'s str>,
) -> fmt::Result {
    let indent = "    ".repeat(depth);
    for statement in statements {
        match statement {
            Statement::Node { id, label, red } => {
                writeln!(f, "{indent}{id}[{}]", quoted(label))?;
                if *red {
                    red_nodes.push(id);
                }
            }
            Statement::Subgraph { id, label, red, inside } => {
                writeln!(f, "{indent}subgraph {id} [{}]", quoted(label))?;
                nodes(f, inside, depth + 1, red_nodes)?;
                writeln!(f, "{indent}end")?;
                if *red {
                    red_nodes.push(id);
                }
            }
            Statement::Comment(_) | Statement::Link { .. } => {}
        }
    }
    Ok(())
}

/// A label as it is when Mermaid reads it so, else in quotes.
fn quoted(label: &str) -> String {
    if label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { label.to_owned() } else { format!("\"{label}\"") }
}

/// The module graph of every crate as a Mermaid flowchart, each crate and each module that has others below it a box
/// around them. The graph links only siblings, so a use of a module's child shows as a use of the box.
/// Two modules that use each other share one bold `<==>` link, another use in a cycle is
/// a bold `==>`, a call up a dashed link, a use of a later layer a labeled bold link, and a module with items in a cycle a red box, all red. With `root`, only that
/// module and those below it. Always a chart Mermaid can draw, even of no module.
pub(crate) fn flowchart(modules: &Graph, problems: &Problems, root: Option<&str>) -> String {
    let mut chart: Vec<Statement> = problems.report.iter().cloned().map(Statement::Comment).collect();
    let below =
        |module: &str| root.is_none_or(|r| module == r || module.strip_prefix(r).is_some_and(|m| m.starts_with("::")));
    let named = modules.iter().flat_map(|(from, tos)| tos.keys().chain([from])).map(String::as_str);
    let crates = named.clone().map(|m| m.split("::").next().unwrap_or(m));
    let nodes: BTreeSet<&str> = named.chain(crates).filter(|m| below(m)).collect();
    if nodes.is_empty() {
        let label = format!("no module {}", root.unwrap_or_default());
        chart.push(Statement::Node { id: "missing".into(), label, red: false });
        return Chart(chart).to_string();
    }
    let (mut children, mut tops) = (BTreeMap::<&str, Vec<&str>>::new(), Vec::new());
    for &node in &nodes {
        let mut ancestors = std::iter::successors(Some(node), |m| Some(m.rsplit_once("::")?.0)).skip(1);
        match ancestors.find(|a| nodes.contains(a)) {
            Some(parent) => children.entry(parent).or_default().push(node),
            None => tops.push(node),
        }
    }
    let around = |item: &String| {
        let mut ancestors = std::iter::successors(Some(item.as_str()), |m| Some(m.rsplit_once("::")?.0)).skip(1);
        ancestors.find_map(|m| nodes.get(m).copied())
    };
    let red: BTreeSet<&str> = problems.items.iter().filter_map(around).collect();
    chart.extend(tops.into_iter().map(|top| node(top, &children, &red)));
    let shown = |from: &str, to: &str| nodes.contains(from) && nodes.contains(to);
    for (from, to) in modules.iter().flat_map(|(from, tos)| tos.keys().map(move |to| (from, to))) {
        let back = modules.get(to).is_some_and(|tos| tos.contains_key(from));
        let arrow = match back {
            // Drawn once, from the first of the two.
            true if to < from => continue,
            true => Arrow::Both,
            false if problems.cyclic.contains(&(from.clone(), to.clone())) => Arrow::Bold,
            false => Arrow::Plain,
        };
        if shown(from, to) {
            chart.push(Statement::Link { from: id(from), arrow, to: id(to) });
        }
    }
    for (from, to) in problems.upcalls.iter().filter(|(from, to)| shown(from, to)) {
        chart.push(Statement::Link { from: id(from), arrow: Arrow::CallUp, to: id(to) });
    }
    for (from, to) in problems.layers.iter().filter(|(from, to)| shown(from, to)) {
        chart.push(Statement::Link { from: id(from), arrow: Arrow::Layer, to: id(to) });
    }
    Chart(chart).to_string()
}

/// A module, or a box around the modules below it, named as in its parent.
fn node(module: &str, children: &BTreeMap<&str, Vec<&str>>, red: &BTreeSet<&str>) -> Statement {
    let (id, label, red_box) =
        (id(module), module.rsplit("::").next().unwrap_or(module).to_owned(), red.contains(module));
    match children.get(module) {
        Some(inner) => Statement::Subgraph {
            id,
            label,
            red: red_box,
            inside: inner.iter().map(|child| node(child, children, red)).collect(),
        },
        None => Statement::Node { id, label, red: red_box },
    }
}

/// The whole path, so that two crates' modules of the same name differ: `shop::ir` is `shop__ir`.
fn id(module: &str) -> String {
    module.replace("::", "__")
}

#[cfg(test)]
mod tests {
    use super::{
        super::graph::{Location, add},
        *,
    };

    fn modules(edges: &[(&str, &str)]) -> Graph {
        let mut graph = Graph::new();
        for (line, (from, to)) in (1..).zip(edges) {
            add(&mut graph, (*from).into(), (*to).into(), Location { file: "src/lib.rs".into(), line });
        }
        graph
    }

    #[test]
    fn a_flowchart_boxes_a_crate_and_a_module_around_their_children() {
        let graph = modules(&[("shop::ir", "shop::ir::types"), ("shop::emit", "shop::ir"), ("shop::emit", "shop::ir")]);
        let problems = Problems {
            upcalls: [("shop::ir::types".into(), "shop::ir".into())].into(),
            items: ["shop::emit::Order".into()].into(),
            report: vec!["functions that call their module's ancestors' free functions:\n  a -> b".into()],
            ..Problems::default()
        };
        assert_eq!(
            flowchart(&graph, &problems, None),
            "flowchart LR
    %% functions that call their module's ancestors' free functions:
    %%   a -> b
    subgraph shop [shop]
        shop__emit[emit]
        subgraph shop__ir [ir]
            shop__ir__types[types]
        end
    end
    shop__emit      -->            shop__ir
    shop__ir        -->            shop__ir__types
    shop__ir__types -.->|calls up| shop__ir
    linkStyle 2 stroke:red,stroke-width:3px
    style shop__emit stroke:red,stroke-width:3px
"
        );
    }

    #[test]
    fn modules_in_a_cycle_are_bold_and_red() {
        let graph =
            modules(&[("shop::a", "shop::b"), ("shop::b", "shop::a"), ("shop::b", "shop::c"), ("shop::c", "shop::a")]);
        let cyclic = [("shop::a", "shop::b"), ("shop::b", "shop::a"), ("shop::b", "shop::c"), ("shop::c", "shop::a")];
        let problems = Problems { cyclic: cyclic.map(|(f, t)| (f.into(), t.into())).into(), ..Problems::default() };
        let chart = flowchart(&graph, &problems, None);
        let links: Vec<&str> = chart.lines().filter(|l| l.contains("=>") || l.contains("linkStyle")).collect();
        assert_eq!(
            links,
            [
                "    shop__a <==> shop__b",
                "    shop__b ==>  shop__c",
                "    shop__c ==>  shop__a",
                "    linkStyle 0,1,2 stroke:red,stroke-width:3px",
            ]
        );
    }

    #[test]
    fn a_root_keeps_the_modules_below_it() {
        let graph = modules(&[("shop::ir", "shop::ir::types"), ("shop::emit", "shop::ir")]);
        assert_eq!(
            flowchart(&graph, &Problems::default(), Some("shop::ir")),
            "flowchart LR\n    subgraph shop__ir [ir]\n        shop__ir__types[types]\n    end\n    shop__ir --> shop__ir__types\n"
        );
        assert_eq!(
            flowchart(&graph, &Problems::default(), Some("shop::cart")),
            "flowchart LR\n    missing[\"no module shop::cart\"]\n"
        );
    }
}
