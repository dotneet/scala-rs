//! Reification: lowering a parsed quasiquote body to universe calls.
//!
//! nsc's quasiquote macros do exactly one thing (`docs/macros.md` §6.2): they
//! parse the interpolation and rewrite it into calls on
//! `<universe>.internal.reificationSupport.Syntactic*`, which build the reflect
//! `Tree` at run time. `crates/typer/src/quasiquote.rs` does the parsing; this
//! module does the rewriting.
//!
//! What comes out is an ordinary **untyped scala-rs tree**, typed and compiled
//! like any other expression. `q"f(1)"` becomes, in source terms:
//!
//! ```text
//! u.internal.reificationSupport.SyntacticApplied(
//!   u.internal.reificationSupport.SyntacticTermIdent(u.TermName("f"), false),
//!   List(List(u.Literal(u.Constant(1)))))
//! ```
//!
//! **The subset is deliberate and every gap is an error.** A form this module
//! does not build is reported as `unimplemented syntax: quasiquote q"..."`
//! naming the form, never quietly dropped or approximated -- a quasiquote that
//! silently built the wrong tree would be far worse than one that does not
//! compile. `Err` carries the reason.

use scala_rs_parser::{Lit, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::quasiquote::{hole_index, QuasiKind};

/// Lowers one quasiquote.
pub(crate) struct Reifier<'a> {
    /// The expression naming the universe (`scala.reflect.runtime.universe`,
    /// `c.universe`, ...), already typed; cloned wherever a receiver is needed.
    universe: Tree,
    /// The interpolation's arguments, in order: `args[i]` fills `$`-hole `i`.
    args: &'a [Tree],
    /// `ranks[i]` is 0 for `$x`, 1 for `..$xs`, 2 for `...$xss`.
    ranks: &'a [u8],
    /// The quasiquote's own span, worn by everything this builds.
    span: Span,
}

impl<'a> Reifier<'a> {
    pub(crate) fn new(universe: Tree, args: &'a [Tree], ranks: &'a [u8], span: Span) -> Self {
        Reifier {
            universe,
            args,
            ranks,
            span,
        }
    }

    /// Lower a quasiquote body. Only `q"..."` is built so far.
    pub(crate) fn reify(&self, kind: QuasiKind, body: &Tree) -> Result<Tree, String> {
        match kind {
            QuasiKind::Term => self.term(body),
            QuasiKind::Type => Err("tq\"...\" is not reified yet".to_string()),
            QuasiKind::Pattern => Err("pq\"...\" is not reified yet".to_string()),
            QuasiKind::Case => Err("cq\"...\" is not reified yet".to_string()),
        }
    }

    /// Lower one term of the body.
    fn term(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Literal { lit } => Ok(self.call(
                self.universe_member("Literal"),
                vec![self.call(
                    self.universe_member("Constant"),
                    vec![self.lit(lit.clone())],
                )],
            )),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0),
                None => Ok(self.call(
                    self.support_member("SyntacticTermIdent"),
                    vec![self.term_name(name), self.lit(Lit::Boolean(false))],
                )),
            },
            TreeKind::Select { qual, name } => {
                // `$x.foo` and `a.b`: the qualifier is a term either way.
                let q = self.term(qual)?;
                Ok(self.call(
                    self.support_member("SyntacticSelectTerm"),
                    vec![q, self.term_name(name)],
                ))
            }
            TreeKind::Apply { fun, args } => {
                let f = self.term(fun)?;
                let clause = self.arg_clause(args)?;
                Ok(self.call(
                    self.support_member("SyntacticApplied"),
                    vec![f, self.list(vec![clause])],
                ))
            }
            other => Err(format!("{} is not reified yet", describe(other))),
        }
    }

    /// One parameter clause as a `List[Tree]`.
    ///
    /// Two shapes are built: every argument an ordinary term, or a single
    /// `..$xs` standing for the whole clause. Mixing the two (`f(a, ..$xs)`)
    /// needs a concatenation whose static type has to be right on both sides,
    /// and building it wrong would silently reorder a call's arguments, so it
    /// is refused instead.
    fn arg_clause(&self, args: &[Tree]) -> Result<Tree, String> {
        let ranks: Vec<u8> = args
            .iter()
            .map(|a| match &a.kind {
                TreeKind::Ident { name } => match hole_index(name) {
                    Some(i) => self.ranks.get(i).copied().unwrap_or(0),
                    None => 0,
                },
                _ => 0,
            })
            .collect();
        if ranks.iter().all(|&r| r == 0) {
            let mut out = Vec::new();
            for a in args {
                out.push(self.term(a)?);
            }
            return Ok(self.list(out));
        }
        if args.len() == 1 && ranks[0] == 1 {
            let TreeKind::Ident { name } = &args[0].kind else {
                unreachable!("rank comes from a hole");
            };
            let i = hole_index(name).expect("rank comes from a hole");
            return self.hole(i, 1);
        }
        Err("a `..$` splice mixed with ordinary arguments is not reified yet".to_string())
    }

    /// The expression filling hole `i`, which must have been written at `rank`.
    fn hole(&self, i: usize, rank: u8) -> Result<Tree, String> {
        let got = self.ranks.get(i).copied().unwrap_or(0);
        if got != rank {
            return Err(format!(
                "a rank-{got} hole ({}) cannot stand for {}",
                dots(got),
                match rank {
                    0 => "a single tree",
                    _ => "a list of trees",
                }
            ));
        }
        self.args
            .get(i)
            .cloned()
            .ok_or_else(|| format!("hole {i} has no argument"))
    }

    // -- building blocks ---------------------------------------------------

    fn node(&self, kind: TreeKind) -> Tree {
        Tree {
            id: NodeId(0),
            span: self.span,
            kind,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        }
    }

    fn lit(&self, lit: Lit) -> Tree {
        self.node(TreeKind::Literal { lit })
    }

    fn select(&self, qual: Tree, name: &str) -> Tree {
        self.node(TreeKind::Select {
            qual: Box::new(qual),
            name: name.to_string(),
        })
    }

    fn call(&self, fun: Tree, args: Vec<Tree>) -> Tree {
        self.node(TreeKind::Apply {
            fun: Box::new(fun),
            args,
        })
    }

    /// `<universe>.<name>`.
    fn universe_member(&self, name: &str) -> Tree {
        self.select(self.universe.clone(), name)
    }

    /// `<universe>.internal.reificationSupport.<name>`.
    fn support_member(&self, name: &str) -> Tree {
        let internal = self.universe_member("internal");
        let support = self.select(internal, "reificationSupport");
        self.select(support, name)
    }

    /// `<universe>.TermName("<s>")`.
    fn term_name(&self, s: &str) -> Tree {
        self.call(
            self.universe_member("TermName"),
            vec![self.lit(Lit::String(s.to_string()))],
        )
    }

    /// `List(<elems>)`, or `Nil` when there are none.
    ///
    /// `List()` on its own infers `List[A]` with nothing to solve `A` from,
    /// and the expected type does not reach a method's type parameter yet, so
    /// `q"f()"` came out as `List[List[A]]`. `Nil` is `List[Nothing]`, which
    /// conforms to `List[Tree]` by covariance and needs no inference at all.
    fn list(&self, elems: Vec<Tree>) -> Tree {
        if elems.is_empty() {
            return self.node(TreeKind::Ident {
                name: "Nil".to_string(),
            });
        }
        self.call(
            self.node(TreeKind::Ident {
                name: "List".to_string(),
            }),
            elems,
        )
    }
}

fn dots(rank: u8) -> &'static str {
    match rank {
        0 => "$x",
        1 => "..$xs",
        _ => "...$xss",
    }
}

/// A short name for a form, for the diagnostic. Deliberately coarse: the point
/// is to say *which* construct is missing, not to pretty-print it.
fn describe(k: &TreeKind) -> &'static str {
    match k {
        TreeKind::Block { .. } => "a block",
        TreeKind::Function { .. } => "a function literal",
        TreeKind::New { .. } => "`new`",
        TreeKind::Typed { .. } => "a type ascription",
        TreeKind::TypeApply { .. } => "a type application",
        TreeKind::If { .. } => "`if`",
        TreeKind::Match { .. } => "`match`",
        TreeKind::Assign { .. } => "an assignment",
        TreeKind::This { .. } => "`this`",
        TreeKind::Super { .. } => "`super`",
        TreeKind::ValDef { .. } => "a `val` definition",
        TreeKind::DefDef { .. } => "a `def` definition",
        TreeKind::ClassDef { .. } => "a class definition",
        TreeKind::ModuleDef { .. } => "an object definition",
        TreeKind::InterpolatedString { .. } => "a nested interpolation",
        _ => "this form",
    }
}
