//! A SCIP symbol's descriptors, as its [grammar](https://github.com/sourcegraph/scip/blob/main/scip.proto) defines
//! them: `engine/impl#[Engine]due().` is the namespace `engine`, the type `impl`, the type parameter `Engine` and the
//! method `due`. rust-analyzer writes an `impl` block as the type `impl` with its self type, then its trait, as type
//! parameters.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    /// `name/`: a module.
    Namespace,
    /// `name#`.
    Type,
    /// `name.`: a field, variant, constant or static.
    Term,
    /// `name().` or `name(disambiguator).`: a function or method.
    Method,
    /// `[name]`.
    TypeParameter,
    /// `(name)`.
    Parameter,
    /// `name:`.
    Meta,
    /// `name!`.
    Macro,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Descriptor {
    pub(crate) name: String,
    pub(crate) kind: Kind,
}

/// A symbol of the crate, without the scheme, the package and its version.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Symbol(pub(crate) Vec<Descriptor>);

impl Symbol {
    /// The descriptors of `text`, or None if it does not follow the grammar.
    pub(crate) fn parse(mut text: &str) -> Option<Self> {
        let mut descriptors = Vec::new();
        while !text.is_empty() {
            let (descriptor, rest) = descriptor(text)?;
            descriptors.push(descriptor);
            text = rest;
        }
        Some(Self(descriptors))
    }

    /// The modules it is in, or that it is.
    pub(crate) fn modules(&self) -> Vec<&str> {
        self.0.iter().take_while(|d| d.kind == Kind::Namespace).map(|d| d.name.as_str()).collect()
    }

    /// Whether it is a module.
    pub(crate) fn is_module(&self) -> bool {
        self.0.iter().all(|d| d.kind == Kind::Namespace)
    }

    /// Whether it is a function outside any type or `impl` block.
    pub(crate) fn is_free_function(&self) -> bool {
        matches!(&self.0[self.modules().len()..], [Descriptor { kind: Kind::Method, .. }])
    }

    /// The self type's name of a method in an inherent `impl` block, without its generics: `Run` of `impl Run<'a>`.
    pub(crate) fn inherent_self_type(&self) -> Option<&str> {
        match &self.0[self.modules().len()..] {
            [Descriptor { name: block, kind: Kind::Type }, Descriptor { name: ty, kind: Kind::TypeParameter }, ..]
                if block == "impl" && self.0.iter().filter(|d| d.kind == Kind::TypeParameter).count() == 1 =>
            {
                ty.split('<').next()
            }
            _ => None,
        }
    }

    /// The type of that name: `engine/Engine#` for `Engine` in `engine`.
    pub(crate) fn is_type_named(&self, name: &str) -> bool {
        matches!(&self.0[self.modules().len()..], [Descriptor { name: n, kind: Kind::Type }] if n == name)
    }
}

/// As a Rust path: `engine::Engine::due`, an `impl` block as `<Engine as Display>`.
impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        let mut descriptors = self.0.iter().peekable();
        while let Some(d) = descriptors.next() {
            match d.kind {
                Kind::Type if d.name == "impl" => {
                    let mut parameters = Vec::new();
                    while let Some(p) = descriptors.next_if(|p| p.kind == Kind::TypeParameter) {
                        parameters.push(p.name.as_str());
                    }
                    match parameters[..] {
                        [ty] => parts.push(ty.to_owned()),
                        [ty, tr] => parts.push(format!("<{ty} as {tr}>")),
                        _ => parts.push(d.name.clone()),
                    }
                }
                Kind::TypeParameter | Kind::Parameter => {}
                Kind::Namespace | Kind::Type | Kind::Term | Kind::Method | Kind::Meta | Kind::Macro => {
                    parts.push(d.name.clone());
                }
            }
        }
        f.write_str(&parts.join("::"))
    }
}

/// The first descriptor of `text` and what follows it.
fn descriptor(text: &str) -> Option<(Descriptor, &str)> {
    let enclosed = |open: char, close: char, kind: Kind| {
        let (name, rest) = name(text.strip_prefix(open)?)?;
        Some((Descriptor { name, kind }, rest.strip_prefix(close)?))
    };
    match text.chars().next()? {
        '[' => return enclosed('[', ']', Kind::TypeParameter),
        '(' => return enclosed('(', ')', Kind::Parameter),
        _ => {}
    }
    let (name, rest) = name(text)?;
    let mut suffix = rest.chars();
    let kind = match suffix.next()? {
        '/' => Kind::Namespace,
        '#' => Kind::Type,
        '.' => Kind::Term,
        ':' => Kind::Meta,
        '!' => Kind::Macro,
        '(' => {
            // `(disambiguator).`
            let (_, after) = suffix.as_str().split_once(").")?;
            return Some((Descriptor { name, kind: Kind::Method }, after));
        }
        _ => return None,
    };
    Some((Descriptor { name, kind }, suffix.as_str()))
}

/// A simple identifier, or one in backticks, in which a doubled backtick is one.
fn name(text: &str) -> Option<(String, &str)> {
    if let Some(quoted) = text.strip_prefix('`') {
        let mut name = String::new();
        let mut chars = quoted.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '`' if chars.peek().is_some_and(|(_, n)| *n == '`') => {
                    chars.next();
                    name.push('`');
                }
                '`' => return Some((name, quoted.get(i + 1..)?)),
                c => name.push(c),
            }
        }
        return None;
    }
    let end = text.find(|c: char| !(c.is_ascii_alphanumeric() || "_+-$".contains(c))).unwrap_or(text.len());
    let (name, rest) = text.split_at_checked(end)?;
    (end > 0).then(|| (name.to_owned(), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(text: &str) -> Symbol {
        Symbol::parse(text).unwrap()
    }

    #[test]
    fn a_method_of_an_inherent_impl_belongs_to_its_type() {
        let due = symbol("shop/impl#[`Order<'a>`]total().");
        assert_eq!(
            (due.modules(), due.inherent_self_type(), due.is_free_function()),
            (vec!["shop"], Some("Order"), false)
        );
        assert_eq!(due.to_string(), "shop::Order<'a>::total");
    }

    #[test]
    fn a_trait_impl_is_its_own_item() {
        let fmt = symbol("shop/impl#[Order][`Display`]fmt().");
        assert_eq!(fmt.inherent_self_type(), None);
        assert_eq!(fmt.to_string(), "shop::<Order as Display>::fmt");
    }

    #[test]
    fn kinds_of_descriptors() {
        assert!(symbol("shop/cart/").is_module());
        assert!(symbol("shop/checkout().").is_free_function());
        assert!(symbol("shop/checkout(+1).").is_free_function());
        assert!(!symbol("shop/Order#total().").is_free_function());
        assert!(symbol("shop/Order#").is_type_named("Order"));
        assert_eq!(symbol("shop/Order#id.").to_string(), "shop::Order::id");
        assert_eq!(symbol("shop/receipt!").to_string(), "shop::receipt");
        assert_eq!(Symbol::parse("shop/Order"), None);
        assert_eq!(symbol("`a``b`#").0[0].name, "a`b");
    }
}
