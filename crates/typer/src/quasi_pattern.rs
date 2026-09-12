//! Quasiquotes in *pattern* position: `case q"($p) => $f($a)" => …`.
//!
//! The expression side of a quasiquote builds a reflect `Tree`
//! (`crates/typer/src/reify.rs`); the pattern side takes one apart. nsc calls
//! the same compiler-internal macro with `nme.unapply` instead of `nme.apply`
//! and runs an `UnapplyReifier` over the same parsed body, which emits a
//! *pattern* over the universe's own extractors rather than a chain of
//! constructor calls.
//!
//! This module is that pass. `q"($param) => $trans[..$typeArgs]($arg)"`
//! becomes, with `u` the universe in scope,
//!
//! ```text
//! u.Function(_root_.scala.List(param),
//!            u.Apply(u.internal.reificationSupport.SyntacticTypeApplied(trans, typeArgs),
//!                    _root_.scala.List(arg)))
//! ```
//!
//! and `param`, `trans`, `typeArgs`, `arg` are the hole patterns the author
//! wrote, spliced in where they stand. Two points are worth spelling out:
//!
//! * **`SyntacticTypeApplied`, not `u.TypeApply`.** A written type-argument
//!   list in a quasiquote matches a call with *or without* type arguments:
//!   nsc's extractor is total (`unapply` returns `Some`, handing back `Nil`
//!   for a plain `Apply`). Checked on the real thing: for both `q"f(1)"` and
//!   `q"f[Int](1)"`, `Apply(SyntacticTypeApplied(fun, targs), args)` and
//!   `q"$fun[..$targs](..$args)"` answer exactly the same.
//! * **Every name this builds is qualified** -- `u.Apply`, `_root_.scala.List`
//!   -- because a bare `Apply` is resolved lexically, and the file that made
//!   this necessary is in package `cats`, which declares a `cats.Apply` of its
//!   own. A package member outranks a wildcard import (SLS 2), so the bare
//!   name would have bound to the type class and the pattern would have
//!   matched nothing.
//!
//! A body shape this does not cover is an **error** naming the shape, never a
//! silent pass: a quasiquote pattern that quietly matched the wrong trees
//! would make a macro implementation compile and then mis-expand.

use crate::check::Typer;
use crate::quasiquote::{hole_index, QuasiKind};
use crate::symbol::SymKind;
use scala_rs_parser::{Lit, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

/// The pieces of `StringContext(parts).<q|tq|pq|cq>(holes)`, which is the tree
/// `Parser::parse_interpolated_pattern` builds for `case q"..." =>`.
pub(crate) fn split(pat: &Tree) -> Option<(QuasiKind, Vec<String>, Vec<Tree>)> {
    let TreeKind::Apply { fun, args } = &pat.kind else {
        return None;
    };
    let TreeKind::Select { qual, name } = &fun.kind else {
        return None;
    };
    let kind = QuasiKind::of(name)?;
    let TreeKind::Apply {
        fun: sc_apply,
        args: lits,
    } = &qual.kind
    else {
        return None;
    };
    let TreeKind::Select {
        qual: sc,
        name: apply,
    } = &sc_apply.kind
    else {
        return None;
    };
    if apply != "apply" || !matches!(&sc.kind, TreeKind::Ident { name } if name == "StringContext")
    {
        return None;
    }
    let mut parts = Vec::with_capacity(lits.len());
    for l in lits {
        match &l.kind {
            TreeKind::Literal {
                lit: Lit::String(s),
            } => parts.push(s.clone()),
            _ => return None,
        }
    }
    Some((kind, parts, args.clone()))
}

impl Typer {
    /// Rewrite `case q"..." =>` into the pattern it deconstructs to.
    ///
    /// Leaves `pat` alone -- so that ordinary pattern typing runs on it
    /// unchanged -- when this is not a quasiquote, when a user-defined
    /// interpolator of the same name is in scope, or when no universe is.
    /// Only a body shape `Deconstructor` does not cover is reported here; the
    /// other two failures are the ordinary route's to report.
    pub(crate) fn deconstruct_quasiquote_pattern(&mut self, pat: &mut Tree) {
        let Some((kind, parts, holes)) = split(pat) else {
            return;
        };
        if self.quasiquote_pattern_user_extractor(pat) {
            return;
        }
        let Some(universe) = self.universe_in_scope() else {
            return;
        };
        let span = pat.span;
        let (body, src) = match crate::quasiquote::parse_body(kind, &parts, holes.len()) {
            Ok(pair) => pair,
            Err(why) => {
                self.error(
                    span,
                    format!(
                        "unimplemented syntax: quasiquote {}\"...\" ({why}). \
                         See docs/macros.md \u{a7}7.",
                        kind.prefix()
                    ),
                );
                pat.ty = Type::Error;
                return;
            }
        };
        let ranks = crate::quasiquote::hole_ranks(&parts, holes.len());
        let built = {
            let d = Deconstructor::new(universe, &holes, &ranks, span, &src);
            match d.pattern(kind, &body) {
                Ok(t) => t,
                Err(why) => {
                    self.error(
                        span,
                        format!(
                            "unimplemented syntax: quasiquote {}\"...\" in pattern position \
                             ({why}). See docs/macros.md \u{a7}7.",
                            kind.prefix()
                        ),
                    );
                    pat.ty = Type::Error;
                    return;
                }
            }
        };
        *pat = built;
    }

    /// Whether `StringContext(...).q` really names an extractor -- that is,
    /// whether the author defined an interpolator of this name themselves.
    ///
    /// The universe's own `scala.reflect.api.Quasiquotes.Quasiquote` cannot be
    /// the answer: `implicit_class_conversion_from` excludes `scala.*` from
    /// the views read out of a pickle, deliberately (see its comment), so the
    /// reflection quasiquote never resolves as a member and a hit here is
    /// always a user's.
    fn quasiquote_pattern_user_extractor(&mut self, pat: &Tree) -> bool {
        let TreeKind::Apply { fun, .. } = &pat.kind else {
            return false;
        };
        let mut probe = (**fun).clone();
        let mark = self.diags.len();
        self.type_expr(&mut probe, &Type::NoType);
        self.prefer_extractor_object(&mut probe);
        let found = self.diags.len() == mark
            && !probe.sym.is_none()
            && matches!(
                self.st.get(probe.sym).kind,
                SymKind::Module | SymKind::Method | SymKind::Term
            )
            && self
                .st
                .class_sym_of(&probe.ty)
                .is_some_and(|c| self.has_extractor(c));
        self.diags.truncate(mark);
        found
    }

    fn has_extractor(&self, cls: SymbolId) -> bool {
        !self.st.lookup_member(cls, "unapply").is_empty()
            || !self.st.lookup_member(cls, "unapplySeq").is_empty()
    }
}

/// Lowers one quasiquote that stands in pattern position.
pub(crate) struct Deconstructor<'a> {
    /// The expression naming the universe (`c.universe`, ...), already typed.
    universe: Tree,
    /// The hole patterns, in order: `holes[i]` is what `$`-hole `i` wrote.
    holes: &'a [Tree],
    /// `ranks[i]` is 0 for `$x`, 1 for `..$xs`, 2 for `...$xss`.
    ranks: &'a [u8],
    /// The quasiquote's own span, worn by everything this builds.
    span: Span,
    /// The source `crates/typer/src/quasiquote.rs` reconstructed and parsed.
    /// Read for the same reason the expression reifier reads it: the parser
    /// has already turned `a :: b` into `b.::(a)`, and only the text says
    /// which of the two was written.
    src: &'a str,
}

impl<'a> Deconstructor<'a> {
    pub(crate) fn new(
        universe: Tree,
        holes: &'a [Tree],
        ranks: &'a [u8],
        span: Span,
        src: &'a str,
    ) -> Self {
        Deconstructor {
            universe,
            holes,
            ranks,
            span,
            src,
        }
    }

    /// The pattern `body` deconstructs to.
    pub(crate) fn pattern(&self, kind: QuasiKind, body: &Tree) -> Result<Tree, String> {
        match kind {
            // `q"..$stats"` / `q"{ ..$stats }"`: a rank-1 hole standing for the
            // whole body is a block of those statements, and nsc's
            // `SyntacticBlock` also answers for a single statement that was
            // never wrapped in a block -- the same reading the expression side
            // gives it (`Reifier::stats_splice`). The parser folds `{ e }`
            // down to `e`, so both spellings arrive here alike.
            QuasiKind::Term if self.rank_of(body) == 1 => {
                let h = self.hole_index_of(body).unwrap();
                Ok(self.call(
                    self.support_member("SyntacticBlock"),
                    vec![self.hole(h, 1)?],
                ))
            }
            // `q"$t"`: a rank-0 hole standing for the whole body matches any
            // tree, but it still has to *be* a tree. nsc's generated matcher
            // takes a `$u.Tree` parameter, so the hole is typed by it; spliced
            // in bare, the hole would have been an ordinary variable pattern
            // binding the scrutinee at whatever type the scrutinee had, and
            // `case q"$a"` would have matched an `Option[Int]`.
            //
            // Where nsc differs: its matcher *casts* rather than tests, so on
            // a non-tree scrutinee `case q"$a"` throws a `MatchError` from
            // inside the synthetic `unapply` instead of falling through to the
            // next case. A type test is what every nested hole already gets
            // here (`u.Apply(…)` cannot match a `String` either), and the
            // difference is visible only for a scrutinee statically wider than
            // `Tree`, which no macro implementation has.
            QuasiKind::Term if self.rank_of(body) == 0 && self.hole_index_of(body).is_some() => {
                let h = self.hole_index_of(body).unwrap();
                Ok(self.node(TreeKind::Typed {
                    expr: Box::new(self.hole(h, 0)?),
                    tpt: Box::new(self.universe_member("Tree")),
                }))
            }
            QuasiKind::Term => self.term(body),
            QuasiKind::Type | QuasiKind::Pattern | QuasiKind::Case => Err(format!(
                "{}\"...\" is not taken apart in pattern position yet",
                kind.prefix()
            )),
        }
    }

    // -- terms -------------------------------------------------------------

    fn term(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Literal { lit } => Ok(self.constant_pat(lit.clone())),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0),
                None => Ok(self.call(
                    self.universe_member("Ident"),
                    vec![self.name_pat("TermName", name)],
                )),
            },
            TreeKind::Select { qual, name } => {
                // `a :: b` reaches the parser as `b.::(a)`, and nsc does not
                // build a plain selection for it even in a pattern. Refused
                // rather than matched the wrong way round.
                if is_right_associative(name) && self.text(t.span).starts_with(name.as_str()) {
                    return Err(format!(
                        "a right-associative operator (`{name}`) written infix"
                    ));
                }
                let q = self.term(qual)?;
                Ok(self.call(
                    self.universe_member("Select"),
                    vec![q, self.name_pat("TermName", name)],
                ))
            }
            TreeKind::Apply { fun, args } => {
                let callee = self.callee(fun)?;
                let args = self.term_list(args)?;
                Ok(self.call(self.universe_member("Apply"), vec![callee, args]))
            }
            TreeKind::Function { vparams, body } => {
                let ps = self.param_list(vparams)?;
                let b = self.term(body)?;
                Ok(self.call(self.universe_member("Function"), vec![ps, b]))
            }
            TreeKind::Block { stats, expr } => {
                // `q"{ $a; $b }"`: nsc matches a block through
                // `SyntacticBlock`, which also answers for a single statement
                // that was never wrapped in a block at all.
                let mut all: Vec<Tree> = stats.clone();
                if !matches!(
                    expr.kind,
                    TreeKind::Empty | TreeKind::Literal { lit: Lit::Unit }
                ) {
                    all.push((**expr).clone());
                }
                let list = self.term_list(&all)?;
                Ok(self.call(self.support_member("SyntacticBlock"), vec![list]))
            }
            other => Err(format!(
                "{} is not taken apart in pattern position yet",
                describe(other)
            )),
        }
    }

    /// The function of an application, with a written type-argument list
    /// folded into `SyntacticTypeApplied`.
    fn callee(&self, fun: &Tree) -> Result<Tree, String> {
        match &fun.kind {
            TreeKind::TypeApply { fun: inner, args } => {
                let f = self.term(inner)?;
                let targs = self.type_list(args)?;
                Ok(self.call(self.support_member("SyntacticTypeApplied"), vec![f, targs]))
            }
            _ => self.term(fun),
        }
    }

    // -- lists -------------------------------------------------------------

    /// A list of terms, as the pattern matching the `List[Tree]` in that
    /// position.
    fn term_list(&self, items: &[Tree]) -> Result<Tree, String> {
        self.list_of(items, &|t| self.term(t), "an argument")
    }

    /// A list of type arguments.
    fn type_list(&self, items: &[Tree]) -> Result<Tree, String> {
        self.list_of(items, &|t| self.typ(t), "a type argument")
    }

    /// A list of function parameters. Each one is a `ValDef` tree, so only a
    /// hole stands for it here -- a parameter written out (`(x: Int) => …`)
    /// would need the `ValDef` pattern nsc builds, which this does not.
    fn param_list(&self, items: &[Tree]) -> Result<Tree, String> {
        self.list_of(items, &|t| self.param(t), "a parameter")
    }

    /// `_root_.scala.List(p1, …, pn)`, or the hole itself when the whole list
    /// is one `..$xs`.
    ///
    /// A rank-1 hole mixed with written elements (`q"f(..$xs, y)"`) is
    /// refused: matching it means splitting a list at a position, which nsc
    /// does with a dedicated combinator per shape.
    fn list_of(
        &self,
        items: &[Tree],
        each: &dyn Fn(&Tree) -> Result<Tree, String>,
        what: &str,
    ) -> Result<Tree, String> {
        let ranks: Vec<u8> = items.iter().map(|t| self.rank_of(t)).collect();
        if let Some(i) = ranks.iter().position(|&r| r > 0) {
            if items.len() != 1 {
                return Err(format!(
                    "a `..$` hole mixed with other elements in {what} list"
                ));
            }
            let Some(h) = self.hole_index_of(&items[i]) else {
                return Err(format!("a rank-1 {what} that is not a hole"));
            };
            if self.ranks[h] != 1 {
                return Err(format!("a `...$` hole in {what} list"));
            }
            return self.hole(h, 1);
        }
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            out.push(each(it)?);
        }
        if out.is_empty() {
            return Ok(self.scala_member("Nil"));
        }
        Ok(self.call(self.scala_member("List"), out))
    }

    // -- types and parameters ----------------------------------------------

    fn typ(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0),
                None => Ok(self.call(
                    self.universe_member("Ident"),
                    vec![self.name_pat("TypeName", name)],
                )),
            },
            other => Err(format!(
                "{} is not taken apart in a type position yet",
                describe(other)
            )),
        }
    }

    /// One parameter of a function literal.
    ///
    /// `($param) => …` parses as a parameter *named* `qqHole0` with no
    /// declared type, not as an `Ident`, which is how a `ValDef` hole reaches
    /// here. A parameter written out (`(x: Int) => …`) would need the `ValDef`
    /// pattern nsc builds for it, which this does not.
    fn param(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Ident { name } if hole_index(name).is_some() => {
                self.hole(hole_index(name).unwrap(), 0)
            }
            TreeKind::ValDef { name, tpt, .. } if hole_index(name).is_some() => {
                if !matches!(tpt.kind, TreeKind::Empty) {
                    return Err("a spliced parameter with a declared type".to_string());
                }
                self.hole(hole_index(name).unwrap(), 0)
            }
            TreeKind::ValDef { .. } => Err(
                "a parameter written out rather than spliced is not taken apart yet".to_string(),
            ),
            other => Err(format!(
                "{} is not taken apart in a parameter position yet",
                describe(other)
            )),
        }
    }

    // -- holes -------------------------------------------------------------

    /// The hole pattern for hole `i`, which must have rank `want`.
    ///
    /// A hole whose pattern carries a type ascription -- `q"${c: C}"` -- is the
    /// dual of `Liftable` on the expression side: nsc unlifts the matched tree
    /// through an `Unliftable[C]` and reports
    /// `Can't find reflect.runtime.universe.Unliftable[C], consider providing it`
    /// when there is none (`test/files/neg/quasiquotes-unliftable-not-found`).
    /// Unlifting is not implemented, so the ascription is refused by name
    /// rather than taken for an ordinary typed pattern, which would have
    /// accepted the program and then bound a `Tree` to a `C`.
    fn hole(&self, i: usize, want: u8) -> Result<Tree, String> {
        let rank = *self.ranks.get(i).unwrap_or(&0);
        if rank != want {
            return Err(format!(
                "a hole of rank {rank} where rank {want} was expected"
            ));
        }
        let pat = self
            .holes
            .get(i)
            .cloned()
            .ok_or_else(|| "a hole with no pattern".to_string())?;
        if matches!(pat.kind, TreeKind::Typed { .. }) {
            return Err(
                "a hole with a type ascription, which needs an `Unliftable` instance".to_string(),
            );
        }
        Ok(pat)
    }

    fn hole_index_of(&self, t: &Tree) -> Option<usize> {
        match &t.kind {
            TreeKind::Ident { name } => hole_index(name),
            _ => None,
        }
    }

    /// The rank written on whatever hole `t` is, or 0 when it is not one.
    fn rank_of(&self, t: &Tree) -> u8 {
        self.hole_index_of(t)
            .and_then(|i| self.ranks.get(i).copied())
            .unwrap_or(0)
    }

    // -- tree building -----------------------------------------------------

    fn node(&self, kind: TreeKind) -> Tree {
        Tree {
            id: NodeId(0),
            span: self.span,
            kind,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
            byname_thunk: false,
            byname_type_marker: false,
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

    fn universe_member(&self, name: &str) -> Tree {
        self.select(self.universe.clone(), name)
    }

    fn support_member(&self, name: &str) -> Tree {
        let internal = self.universe_member("internal");
        let support = self.select(internal, "reificationSupport");
        self.select(support, name)
    }

    /// `_root_.scala.<name>`, the spelling nsc's own reifier uses so that
    /// nothing in the macro's own package can capture the name.
    fn scala_member(&self, name: &str) -> Tree {
        let root = self.node(TreeKind::Ident {
            name: "_root_".to_string(),
        });
        let scala = self.select(root, "scala");
        self.select(scala, name)
    }

    /// `<universe>.Literal(<universe>.Constant(v))`, as a pattern.
    fn constant_pat(&self, lit: Lit) -> Tree {
        self.call(
            self.universe_member("Literal"),
            vec![self.call(self.universe_member("Constant"), vec![self.lit(lit)])],
        )
    }

    /// `<universe>.TermName("n")` / `<universe>.TypeName("n")`, as a pattern.
    ///
    /// The name is encoded the way nsc's parser encodes it, so `q"$a + $b"`
    /// matches a selection of `$plus` and not of `+`: reflect `Name`s carry
    /// the encoded spelling, and matching the raw one would have matched
    /// nothing at all, silently.
    fn name_pat(&self, which: &str, name: &str) -> Tree {
        self.call(
            self.universe_member(which),
            vec![
                self.lit(Lit::String(scala_rs_pickle::names::encode_method_name(
                    name,
                ))),
            ],
        )
    }

    /// The text the body was parsed from, under `span`.
    fn text(&self, span: Span) -> &str {
        self.src
            .get(span.lo.to_usize()..span.hi.to_usize())
            .unwrap_or("")
    }
}

/// Scala's rule: a method whose name ends in `:` is right-associative.
fn is_right_associative(name: &str) -> bool {
    name.len() > 1 && name.ends_with(':') && !name.ends_with("::=")
}

/// How a tree shape is named in a diagnostic.
fn describe(kind: &TreeKind) -> &'static str {
    match kind {
        TreeKind::New { .. } => "`new C(…)`",
        TreeKind::If { .. } => "an `if`",
        TreeKind::Match { .. } => "a `match` (or a `{ case … }` literal)",
        TreeKind::Assign { .. } => "an assignment",
        TreeKind::Return { .. } => "a `return`",
        TreeKind::Throw { .. } => "a `throw`",
        TreeKind::Try { .. } => "a `try`",
        TreeKind::While { .. } => "a `while`",
        TreeKind::ValDef { .. } => "a `val` definition",
        TreeKind::DefDef { .. } => "a `def` definition",
        TreeKind::TypeApply { .. } => "a type application",
        TreeKind::This { .. } => "`this`",
        TreeKind::Super { .. } => "`super`",
        TreeKind::Typed { .. } => "an ascription",
        TreeKind::Wildcard => "a `_`",
        _ => "this shape",
    }
}
