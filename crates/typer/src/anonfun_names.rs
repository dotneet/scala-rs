//! nsc's names for the anonymous classes of partial-function literals.
//!
//! `{ case ... }` compiles to a class of its own, named after what owns the
//! literal:
//!
//! * `$anonfun$<method>$<n>` when that is a method: a member `def`, a local
//!   `def` (itself renamed `inner$1`), a `lazy val`'s `<name>$lzycompute`, a
//!   default getter `<method>$default$<i>`, or the method a function literal
//!   or by-name argument becomes, spelled `$nestedInanonfun$<method>$<k>`
//!   (`ensureNonAnon`);
//! * `$anonfun$<n>` otherwise (a `val`'s right-hand side, a template
//!   statement).
//!
//! `flatten` joins it to the enclosing class: `pk/M$` and `$anonfun$m0$1`
//! make `pk/M$$anonfun$m0$1`, and a literal inside another's case body is
//! nested in that one's class (`M$$anonfun$h$1$$anonfun$applyOrElse$1`).
//!
//! The counters are the compilation unit's, one per prefix, and the methods
//! function literals become draw on the same ones: `$anonfun$<member>$<n>`,
//! `<member>` the nearest enclosing non-local member (`new` for a template
//! statement). Names are handed out in one pre-order walk of the unit, so
//! `def q2 = xs.collect { case 1 => 2 } ++ xs.map(x => x)` makes the class
//! `$anonfun$q2$1` and the method `$anonfun$q2$2`, and `q3` with the two the
//! other way round the reverse (checked against scalac 2.13.16).
//!
//! The result maps each literal's span to its name *relative to the class it
//! is emitted in*; the backend joins it to that class's binary name.

use crate::erasure::for_each_child;
use crate::symbol::SymbolTable;
use scala_rs_parser::ast::*;
use std::collections::HashMap;

/// Span of a partial-function literal → its class name relative to the
/// enclosing class (`$anonfun$m0$1`).
pub type AnonfunClassNames = rustc_hash::FxHashMap<(u32, u32), String>;

/// nsc's names for the partial-function classes of one compilation unit, from
/// its typed tree.
pub fn anonfun_class_names(unit: &Tree, st: &SymbolTable) -> AnonfunClassNames {
    let mut w = Walk {
        st,
        fresh: HashMap::new(),
        out: AnonfunClassNames::default(),
    };
    w.walk(unit, &At::template());
    w.out
}

/// What a literal's class is owned by when `LambdaLift` names it.
#[derive(Clone)]
enum Owner {
    /// A method, by the name it has by then.
    Method(String),
    /// A value or a template.
    Plain,
}

struct Walk<'a> {
    st: &'a SymbolTable,
    /// The unit's fresh-name counters, by prefix.
    fresh: HashMap<String, u32>,
    out: AnonfunClassNames,
}

fn key(t: &Tree) -> Option<(u32, u32)> {
    (!t.span.is_dummy()).then_some((t.span.lo.0, t.span.hi.0))
}

fn encode(name: &str) -> String {
    scala_rs_pickle::names::encode_method_name(name)
}

/// The name nsc gives a member's default getter for its `i`th parameter
/// (1-based, counted across every parameter list).
fn default_getter(method: &str, i: usize) -> String {
    format!("{}$default${i}", encode(method))
}

/// Where the walk is.
#[derive(Clone)]
struct At {
    /// The nearest enclosing non-local member, encoded: what a function
    /// literal's method is named after.
    member: String,
    /// What a partial function's class would be owned by.
    owner: Owner,
    /// The enclosing partial-function class's relative name, empty outside
    /// one.
    rel: String,
}

impl At {
    fn template() -> At {
        At {
            member: String::new(),
            owner: Owner::Plain,
            rel: String::new(),
        }
    }

    /// A member `def` (or a default getter) of a template.
    fn method(name: String) -> At {
        At {
            member: name.clone(),
            owner: Owner::Method(name),
            rel: String::new(),
        }
    }
}

impl Walk<'_> {
    fn fresh(&mut self, prefix: String) -> String {
        let n = self.fresh.entry(prefix.clone()).or_insert(0);
        *n += 1;
        format!("{prefix}{n}")
    }

    fn is_partial_function(&self, t: &Tree) -> bool {
        self.st.class_sym_of(&t.ty).is_some_and(|c| {
            let s = self.st.get(c);
            s.name == "PartialFunction" && s.jvm_name.contains("PartialFunction")
        })
    }

    fn walk(&mut self, t: &Tree, at: &At) {
        match &t.kind {
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => {
                self.param_defaults(vparamss, "<init>", &At::template());
                self.template(impl_);
            }
            TreeKind::ModuleDef { impl_, .. } => self.template(impl_),
            TreeKind::Function { body, .. } if self.is_partial_function(t) => {
                let own = self.fresh(match &at.owner {
                    Owner::Method(m) => format!("$anonfun${m}$"),
                    Owner::Plain => "$anonfun$".to_string(),
                });
                let rel = if at.rel.is_empty() {
                    own
                } else {
                    format!("{}${own}", at.rel)
                };
                if let Some(k) = key(t) {
                    self.out.insert(k, rel.clone());
                }
                // The cases are the class's `applyOrElse`.
                let inside = At {
                    member: "applyOrElse".to_string(),
                    owner: Owner::Method("applyOrElse".to_string()),
                    rel,
                };
                match pf_cases(body) {
                    Some(cases) => {
                        for c in cases {
                            self.walk(&c.guard, &inside);
                            self.walk(&c.body, &inside);
                        }
                    }
                    None => self.walk(body, &inside),
                }
            }
            // A function literal becomes a method, and by-name arguments are
            // function literals by now (`() => e`).
            TreeKind::Function { .. } => {
                let method = self.fresh(format!("$anonfun${}$", at.member));
                let inside = At {
                    owner: Owner::Method(format!("$nestedIn{}", &method[1..])),
                    ..at.clone()
                };
                for_each_child(t, &mut |c| self.walk(c, &inside));
            }
            // A local `def` is renamed `name$<n>`.
            TreeKind::DefDef {
                name,
                vparamss,
                rhs,
                ..
            } => {
                let local = self.fresh(format!("{}$", encode(name)));
                self.param_defaults(vparamss, name, at);
                let inside = At {
                    owner: Owner::Method(local),
                    ..at.clone()
                };
                self.walk(rhs, &inside);
            }
            // A local `lazy val` has become the local method `name$lzycompute`.
            TreeKind::ValDef {
                mods, name, rhs, ..
            } => {
                let owner = if mods.flags.contains(Flags::LAZY) {
                    Owner::Method(self.fresh(format!("{}$lzycompute$", encode(name))))
                } else {
                    Owner::Plain
                };
                let inside = At {
                    owner,
                    ..at.clone()
                };
                self.walk(rhs, &inside);
            }
            _ => for_each_child(t, &mut |c| self.walk(c, at)),
        }
    }

    /// The default arguments of `method`'s parameters, each in its getter
    /// `method$default$<i>`. A local method's getters are local too, and name
    /// nothing after themselves.
    ///
    /// The typed body is the parameter symbol's `default_rhs`; the tree keeps
    /// the one the parser wrote, which has no types to tell a partial
    /// function by.
    fn param_defaults(&mut self, vparamss: &[Vec<Tree>], method: &str, at: &At) {
        let st = self.st;
        for (i, p) in vparamss.iter().flatten().enumerate() {
            let TreeKind::ValDef { rhs, .. } = &p.kind else {
                continue;
            };
            let rhs = match (!p.sym.is_none()).then(|| st.get(p.sym).default_rhs.as_ref()) {
                Some(Some(typed)) => typed,
                _ => &**rhs,
            };
            let inside = if at.rel.is_empty() && at.member.is_empty() {
                At::method(default_getter(method, i + 1))
            } else {
                at.clone()
            };
            self.walk(rhs, &inside);
        }
    }

    fn template(&mut self, impl_: &Template) {
        for s in &impl_.body {
            match &s.kind {
                TreeKind::DefDef {
                    name,
                    vparamss,
                    rhs,
                    ..
                } => {
                    self.param_defaults(vparamss, name, &At::template());
                    let mut at = At::method(encode(name));
                    if name == "<init>" {
                        at.member = "new".to_string();
                    }
                    self.walk(rhs, &at);
                }
                TreeKind::ValDef {
                    mods, name, rhs, ..
                } => {
                    let owner = if mods.flags.contains(Flags::LAZY) {
                        Owner::Method(format!("{}$lzycompute", encode(name)))
                    } else {
                        Owner::Plain
                    };
                    let at = At {
                        member: encode(name),
                        owner,
                        rel: String::new(),
                    };
                    self.walk(rhs, &at);
                }
                TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
                    self.walk(s, &At::template())
                }
                _ => {
                    let at = At {
                        member: "new".to_string(),
                        ..At::template()
                    };
                    self.walk(s, &at);
                }
            }
        }
    }
}

/// The cases of a partial-function literal's body.
fn pf_cases(body: &Tree) -> Option<&[CaseDef]> {
    match &body.kind {
        TreeKind::Match { cases, .. } => Some(cases),
        TreeKind::Block { expr, .. } => pf_cases(expr),
        _ => None,
    }
}
