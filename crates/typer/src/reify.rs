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
//!
//! Every shape below was read off real scalac 2.13.16 with
//! `-Ymacro-debug-lite`, which prints the expansion nsc's own quasiquote macro
//! produces; `tests/fixtures/reify_qq.scala` and `qq_universe.scala` then
//! compare `showRaw` of the result.

use std::cell::RefCell;

use scala_rs_parser::{CaseDef, Flags, Lit, Modifiers, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_pickle::names::encode_method_name;
use scala_rs_span::Span;

use crate::quasiquote::{hole_index, QuasiKind};
use crate::uncurry::is_eta_marker;

/// Definitions -- `class`, `trait`, `object`, `def`, `val` -- live in their
/// own file. It is a child module rather than a sibling so it can use the
/// building blocks below without any of them becoming visible crate-wide.
#[path = "reify_defs.rs"]
mod defs;

/// How a hole's argument becomes a reflect `Tree`.
///
/// nsc infers an implicit `Liftable[T]` for a hole whose argument is not a
/// `Tree` and splices `Liftable.liftX[T](arg)`
/// (`scala/reflect/api/StandardLiftables.scala`). scala-rs picks the standard
/// instance by the argument's type -- `crate::check::Check::lift_for` -- and
/// builds the *same tree that instance builds*, which is what the dual run
/// against real scalac in `tests/fixtures/lf2_lift.scala` compares. A hole
/// whose type has no standard instance is `Unknown` and is diagnosed; a
/// user-written `Liftable` is not searched for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Lift {
    /// Already a `Tree` (`liftTree`, the identity): splice it.
    Tree,
    /// A `Name`. Which tree it becomes depends on the position it stands in.
    Name,
    /// A primitive or `String` (`liftInt` &co): `u.Literal(u.Constant(v))`.
    Value,
    /// A `Constant` (`liftConstant`): `u.Literal(c)`.
    Constant,
    /// A `Type` (`liftType`): `rs.mkTypeTree(t)`.
    Type,
    /// A `WeakTypeTag` / `TypeTag` (`liftTypeTag`): `rs.mkTypeTree(tag.tpe)`.
    TypeTag,
    /// An `Expr[T]` (`liftExpr`): `e.tree`.
    Expr,
    /// A `Symbol`: `rs.mkRefTree(u.EmptyTree, sym)`. Not a `Liftable` at all
    /// -- nsc special-cases the hole -- so, unlike the rest, it cannot appear
    /// under `..$` (nsc says "consider omitting the dots or providing an
    /// implicit instance of `Liftable[Symbol]`" there).
    Symbol,
    /// A rank-1 (`..$xs`) hole over elements that themselves need lifting.
    /// `to_list` is set when the collection is not already a `List`, which is
    /// how nsc writes it (`xs.toList.map(x => lift(x))`).
    Elems { to_list: bool, elem: Box<Lift> },
    /// No standard `Liftable`; the string is the type, for the diagnostic.
    Unknown(String),
}

/// Where a hole stands, which decides what a `Name` becomes there.
#[derive(Clone, Copy, PartialEq)]
enum Pos {
    Term,
    Type,
    Pat,
    /// A name slot of an enclosing form (`q"$x.$n"`, `q"val $n = e"`): the
    /// argument is the name itself, so nothing is built around it.
    Name,
}

/// The `Modifiers` flag set nsc gives a function literal's parameter
/// (`Flags.PARAM`, `1 << 13`). Read off `-Ymacro-debug-lite` for
/// `q"(y: Int) => y"`, which reifies the parameter as
/// `SyntacticValDef(Modifiers(FlagsRepr(8192L), TypeName(""), List()), ...)`.
const PARAM_FLAGS: i64 = 8192;

/// `Flags.PARAM | Flags.SYNTHETIC` (`1 << 13 | 1 << 21`), the modifiers nsc
/// gives the parameter it invents for a `_` placeholder lambda. Read off
/// `-Ymacro-debug-lite` for `q"_.get"`.
const PLACEHOLDER_PARAM_FLAGS: i64 = 2105344;

/// `Flags.DEFERRED | Flags.SYNTHETIC` (`1 << 4 | 1 << 21`), the modifiers on
/// the `TypeDef` an existential's bound type parameter gets. Read off
/// `-Ymacro-debug-lite` for `tq"P[_, _]"`.
const EXISTENTIAL_TPARAM_FLAGS: i64 = 2097168;

/// `Flags.FINAL | Flags.SYNTHETIC | Flags.ARTIFACT`
/// (`1 << 5 | 1 << 21 | 1L << 46`), the modifiers on the `val` nsc binds the
/// left operand of a right-associative operator to. Read off
/// `-Ymacro-debug-lite` for `q"a :: b"`.
const RASSOC_VAL_FLAGS: i64 = 70368746274848;

/// The fresh names a body needs, and what they stand for while it is built.
///
/// nsc's quasiquote macro does not invent names itself: it emits
/// `val nn$macro$k = <universe>.internal.reificationSupport.freshTermName("x$")`
/// into a **block around the whole expansion** and uses `nn$macro$k` wherever
/// the name is needed, so the reflect `Tree` gets a name drawn from the
/// universe's own counter at run time. Three forms need this -- a `_`
/// placeholder lambda, a `_` type argument (an existential) and a
/// right-associative operator -- and all three hoist to the same block, which
/// is why this is per-`Reifier` state rather than something `term` returns.
#[derive(Default)]
struct Fresh {
    /// The `val nn$macro$k = rs.fresh{Term,Type}Name("...")` bindings, in the
    /// order they were asked for; `reify` wraps the body in a block of them.
    defs: Vec<Tree>,
    /// How many have been handed out, for the local's name.
    next: usize,
    /// Placeholder parameters in scope, innermost last: the parser's invented
    /// `x$n` mapped to the local holding its fresh `TermName`. An `Ident` of
    /// one of these in the body is that parameter, not a name to build.
    params: Vec<(String, String)>,
    /// `_` type arguments in scope, keyed by the node the parser made for
    /// them: the local holding the fresh `TypeName` the existential binds.
    wilds: Vec<(NodeId, String)>,
    /// How deep inside a pattern the walk is. A `_` type argument is an
    /// existential only in a *type*; in a pattern (`pq"_: R[_, _]"`) nsc
    /// leaves it as `Bind(TypeName("_"), EmptyTree)` and needs no fresh name.
    pat_depth: usize,
}

/// How one identifier of a `reify { … }` body is rebuilt inside the
/// `TreeCreator`, which is the whole of reify's **hygiene**.
///
/// A quasiquote reifies a name as the name that was written
/// (`SyntacticTermIdent(TermName("f"), false)`), and the tree it builds means
/// whatever `f` means where the tree is finally typed. `reify` must not do
/// that: the expression was written in the macro implementation's scope and
/// has to keep meaning what it meant there, wherever the expansion lands. So
/// nsc reifies each reference by its *symbol*, and this is the subset
/// scala-rs builds -- see `docs/macros.md` §7.14 and `tests/fixtures/
/// rd_impl.scala`, which is this same shape written out by hand.
///
/// Everything else -- a local, a parameter, `this` -- is **refused by name**.
/// nsc turns those into free terms carried in the expansion; building the
/// bare name instead would silently capture whatever stands there at the call
/// site, which is exactly the bug reification exists to prevent.
#[derive(Clone)]
pub(crate) enum ReifyRef {
    /// A static `object`: `rs.mkIdent($m.staticModule("<full name>"))`.
    StaticModule(String),
    /// `x.splice`: the argument's own tree, rebased into the mirror the
    /// creator was handed -- `x.in[$u.type]($m).tree`. Carries the `x`.
    Splice(Box<Tree>),
    /// A type argument, rebuilt inside the creator and wrapped in
    /// `rs.mkTypeTree(...)` -- which is `TypeTree().setType(...)`, the tree
    /// nsc's reifier puts in a type position.
    ///
    /// The type itself is built the way a `TypeTag` is
    /// (`crate::materialize::TagBody`): a monomorphic class is one
    /// `staticClass` call, a type constructor at arguments is `appliedType`
    /// over those, and an abstract type is only knowable through a tag in
    /// scope -- which is the case slick's `TableQueryMacroImpl` needs, where
    /// `reify { TableQuery.apply[E](cons.splice) }` reaches `E` through the
    /// implicit `c.WeakTypeTag[E]`.
    Type(Box<crate::materialize::TagBody>),
    /// A type argument whose type scala-rs cannot rebuild, carrying the
    /// reason the tag builder gave. Kept as a classification of its own so
    /// the report says *which* type and why, rather than "a type in a reify
    /// body is not reified yet".
    TypeGap(String),
}

/// What a `reify { … }` body needs beyond a quasiquote's.
pub(crate) struct ReifyCtx {
    /// The classification of each identifier of the body, by node.
    ///
    /// Built by `Check::reify_refs`, which types a *clone* of each candidate
    /// to find out what it means -- the same speculative shape
    /// `Check::hole_lifts` uses. Nodes the walk did not classify are the ones
    /// refused above.
    pub(crate) refs: std::collections::HashMap<NodeId, ReifyRef>,
    /// The local the creator binds the mirror to, cast to
    /// `Mirror[$u.type]` (`docs/macros.md` §7.14, item 3).
    pub(crate) mirror_local: String,
    /// The local the creator binds `$m$untyped.universe` to. `Reifier`'s
    /// `universe` is an `Ident` of it; the name is needed again for the
    /// `$u.type` in `x.in[$u.type]($m)`.
    pub(crate) universe_local: String,
}

/// Lowers one quasiquote.
pub(crate) struct Reifier<'a> {
    /// The expression naming the universe (`scala.reflect.runtime.universe`,
    /// `c.universe`, ...), already typed; cloned wherever a receiver is needed.
    universe: Tree,
    /// The interpolation's arguments, in order: `args[i]` fills `$`-hole `i`.
    args: &'a [Tree],
    /// `ranks[i]` is 0 for `$x`, 1 for `..$xs`, 2 for `...$xss`.
    ranks: &'a [u8],
    /// `lifts[i]` says how `args[i]` becomes a `Tree`; see `Lift`.
    lifts: &'a [Lift],
    /// The quasiquote's own span, worn by everything this builds.
    span: Span,
    /// The source `crates/typer/src/quasiquote.rs` reconstructed and parsed,
    /// which the spans in the body index into.
    ///
    /// Needed because the parser folds two different types into one node:
    /// `A => B` and `Function1[A, B]` both come out as
    /// `AppliedTypeTree(Ident("Function1"), ...)`, and nsc builds *different*
    /// trees for them (`_root_.scala.Function1` for the arrow,
    /// a bare `Ident` for the written name). The text under the head's span
    /// settles which was written.
    src: &'a str,
    /// The fresh names the body needs; see `Fresh`. Interior mutability
    /// because building a tree is otherwise a pure `&self` walk and only
    /// these three forms have to reach back out of it.
    fresh: RefCell<Fresh>,
    /// Set when this is a `reify { … }` body rather than a quasiquote; see
    /// `ReifyCtx`.
    reify: Option<ReifyCtx>,
}

impl<'a> Reifier<'a> {
    pub(crate) fn new(
        universe: Tree,
        args: &'a [Tree],
        ranks: &'a [u8],
        lifts: &'a [Lift],
        span: Span,
        src: &'a str,
    ) -> Self {
        Reifier {
            universe,
            args,
            ranks,
            lifts,
            span,
            src,
            fresh: RefCell::new(Fresh::default()),
            reify: None,
        }
    }

    /// Turn this into the reifier for a `reify { … }` body.
    pub(crate) fn in_reify(mut self, ctx: ReifyCtx) -> Self {
        self.reify = Some(ctx);
        self
    }

    /// Lower a quasiquote body, in the block of fresh-name bindings it needs.
    ///
    /// The block is nsc's own shape: every `freshTermName` / `freshTypeName`
    /// the body asked for is bound once, ahead of the expression that builds
    /// the tree, so a name used twice (a placeholder parameter and its
    /// occurrences) is the *same* name.
    pub(crate) fn reify(&self, kind: QuasiKind, body: &Tree) -> Result<Tree, String> {
        let built = self.reify_body(kind, body)?;
        let defs = std::mem::take(&mut self.fresh.borrow_mut().defs);
        if defs.is_empty() {
            return Ok(built);
        }
        Ok(self.node(TreeKind::Block {
            stats: defs,
            expr: Box::new(built),
        }))
    }

    fn reify_body(&self, kind: QuasiKind, body: &Tree) -> Result<Tree, String> {
        match kind {
            // `q"..$stats"` / `q"{ ..$stats }"`: a rank-1 hole standing for
            // the whole body is a block of those statements. The parser folds
            // `{ e }` down to `e`, so both spellings arrive here alike.
            QuasiKind::Term => match self.stats_splice(std::slice::from_ref(body))? {
                Some(t) => Ok(t),
                None => self.term(body),
            },
            QuasiKind::Type => self.typ(body),
            QuasiKind::Pattern => self.pat(body),
            QuasiKind::Case => match &body.kind {
                TreeKind::Match { cases, .. } if cases.len() == 1 => self.case_def(&cases[0]),
                TreeKind::Match { .. } => {
                    Err("cq\"...\" holds more than one case clause".to_string())
                }
                other => Err(format!("{} is not reified yet", describe(other))),
            },
        }
    }

    // -- terms -------------------------------------------------------------

    /// Lower one term of the body.
    fn term(&self, t: &Tree) -> Result<Tree, String> {
        if let Some(t) = self.reify_term(t)? {
            return Ok(t);
        }
        // `new C(a)(b)`: the parser leaves an application spine whose head is
        // `New`, and nsc puts *every* clause inside the one `SyntacticNew`.
        if let Some(t) = self.new_spine(t)? {
            return Ok(t);
        }
        match &t.kind {
            TreeKind::Literal { lit } => Ok(self.constant(lit.clone())),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0, Pos::Term),
                // An occurrence of a `_` placeholder's parameter is the fresh
                // name the enclosing `SyntacticFunction` binds, not a name
                // built from the one the parser invented.
                None => Ok(match self.placeholder_param(name) {
                    Some(local) => self.call(
                        self.support_member("SyntacticTermIdent"),
                        vec![self.local(&local), self.lit(Lit::Boolean(false))],
                    ),
                    None => self.term_ident(name),
                }),
            },
            TreeKind::Select { qual, name } => {
                // `$x.foo` and `a.b`: the qualifier is a term either way.
                let q = self.term(qual)?;
                Ok(self.select_term(q, name, t.span)?)
            }
            TreeKind::Apply { fun, args } => {
                // `a :: b`: a right-associative operator, whose left operand
                // nsc binds to a fresh `val` first.
                if let Some(t) = self.right_assoc(fun, args)? {
                    return Ok(t);
                }
                // `(a, b)` is `Apply(Ident("TupleN"), ...)` after parsing,
                // exactly like a written `TupleN(a, b)`, and nsc builds
                // different trees for the two. The source text under the
                // head's span says which was written -- the same reading the
                // arrow type needs.
                if let TreeKind::Ident { name } = &fun.kind {
                    if is_tuple_name(name, args.len()) && self.text(fun.span) != name.as_str() {
                        let mut es = Vec::new();
                        for a in args {
                            es.push(self.term(a)?);
                        }
                        return Ok(
                            self.call(self.support_member("SyntacticTuple"), vec![self.list(es)])
                        );
                    }
                }
                let f = self.term(fun)?;
                let clause = self.arg_clause(args)?;
                Ok(self.call(
                    self.support_member("SyntacticApplied"),
                    vec![f, self.list(vec![clause])],
                ))
            }
            TreeKind::TypeApply { fun, args } => {
                let f = self.term(fun)?;
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.typ(a)?);
                }
                Ok(self.call(
                    self.support_member("SyntacticTypeApplied"),
                    vec![f, self.list(ts)],
                ))
            }
            // `f _`: an eta expansion, which nsc writes as an ascription to
            // the "function" type marker `SyntacticFunction(Nil, EmptyTree)`.
            TreeKind::Typed { expr, tpt } if is_eta_marker(tpt) => {
                let e = self.term(expr)?;
                let marker = self.call(
                    self.support_member("SyntacticFunction"),
                    vec![self.list(vec![]), self.universe_member("EmptyTree")],
                );
                Ok(self.call(self.universe_member("Typed"), vec![e, marker]))
            }
            TreeKind::Typed { expr, tpt } => {
                let e = self.term(expr)?;
                let ty = self.typ(tpt)?;
                Ok(self.call(self.universe_member("Typed"), vec![e, ty]))
            }
            TreeKind::New { .. } => unreachable!("handled by new_spine"),
            TreeKind::Block { stats, expr } => {
                let mut elems = stats.clone();
                if !matches!(&expr.kind, TreeKind::Empty) {
                    elems.push((**expr).clone());
                }
                if let Some(t) = self.stats_splice(&elems)? {
                    return Ok(t);
                }
                let mut out = Vec::new();
                for s in &elems {
                    out.push(self.stat(s)?);
                }
                Ok(self.call(self.support_member("SyntacticBlock"), vec![self.list(out)]))
            }
            TreeKind::Function { vparams, body } => self.function(vparams, body),
            TreeKind::Match { selector, cases } => {
                let sel = self.term(selector)?;
                let cs = self.case_defs(cases)?;
                Ok(self.call(
                    self.support_member("SyntacticMatch"),
                    vec![sel, self.list(cs)],
                ))
            }
            // `if (c) t` has no `else` in the source, and the parser supplies
            // the unit literal for it. nsc supplies `SyntacticBlock(Nil)`
            // instead, and the two are different trees, so an `if` whose
            // `else` is `()` -- written or supplied -- is refused rather than
            // guessed at.
            TreeKind::If { cond, thenp, elsep } => {
                if matches!(&elsep.kind, TreeKind::Literal { lit: Lit::Unit }) {
                    return Err("an `if` without an `else` is not reified yet".to_string());
                }
                let c = self.term(cond)?;
                let a = self.term(thenp)?;
                let b = self.term(elsep)?;
                Ok(self.call(self.universe_member("If"), vec![c, a, b]))
            }
            TreeKind::Assign { lhs, rhs } => {
                let l = self.term(lhs)?;
                let r = self.term(rhs)?;
                Ok(self.call(self.support_member("SyntacticAssign"), vec![l, r]))
            }
            TreeKind::This { qual } => Ok(self.call(
                self.universe_member("This"),
                vec![self.type_name(qual.as_deref().unwrap_or(""))],
            )),
            TreeKind::Super { qual, mix } => Ok(self.super_ref(qual.as_deref(), mix.as_deref())),
            TreeKind::ValDef { .. }
            | TreeKind::DefDef { .. }
            | TreeKind::ClassDef { .. }
            | TreeKind::ModuleDef { .. } => self.definition(t),
            other => Err(format!("{} is not reified yet", describe(other))),
        }
    }

    /// The reify-only reading of one term: `Ok(None)` when the ordinary walk
    /// below should take it.
    ///
    /// Two things happen here and nowhere else. A node the classification
    /// resolved is built from its *symbol* rather than from the name that was
    /// written, and every remaining form is checked against the subset reify
    /// builds -- so an unclassified `Ident` is a local, and is refused by
    /// name rather than reified as the bare name it happens to carry.
    fn reify_term(&self, t: &Tree) -> Result<Option<Tree>, String> {
        let Some(ctx) = &self.reify else {
            return Ok(None);
        };
        if let Some(r) = ctx.refs.get(&t.id) {
            return Ok(Some(match r {
                ReifyRef::StaticModule(name) => self.call(
                    self.support_member("mkIdent"),
                    vec![self.call(
                        self.select(self.local(&ctx.mirror_local), "staticModule"),
                        vec![self.lit(Lit::String(name.clone()))],
                    )],
                ),
                ReifyRef::Splice(e) => self.splice_tree(ctx, e),
                ReifyRef::Type(body) => self.call(
                    self.support_member("mkTypeTree"),
                    vec![self.rebuild_type(ctx, body)],
                ),
                ReifyRef::TypeGap(why) => {
                    return Err(format!("a type argument cannot be rebuilt: {why}"))
                }
            }));
        }
        match &t.kind {
            // The forms whose *parts* are what carry meaning: each is walked
            // on and its own leaves are classified or refused.
            TreeKind::Literal { .. }
            | TreeKind::Select { .. }
            | TreeKind::Apply { .. }
            | TreeKind::TypeApply { .. }
            | TreeKind::If { .. } => Ok(None),
            TreeKind::Ident { name } => Err(format!(
                "`{name}` is a local, a parameter, or a name that does not stand for \
                 a static `object`"
            )),
            other => Err(format!("{} is not reified yet", describe(other))),
        }
    }

    /// One type, rebuilt inside the creator; see `ReifyRef::Type`.
    ///
    /// The same three shapes `crate::materialize` builds, written against the
    /// creator's *cast* mirror rather than its parameter: `$m` is
    /// `Mirror[$u.type]`, so `$m.staticClass(n)` is a `$u.ClassSymbol` and the
    /// result is a `$u.Type` -- which is what `mkTypeTree` and the tree being
    /// built around it want. The materialiser's own creator can select on the
    /// parameter directly because its result is erased to `Types$TypeApi` and
    /// nothing further is built on it.
    fn rebuild_type(&self, ctx: &ReifyCtx, body: &crate::materialize::TagBody) -> Tree {
        use crate::materialize::TagBody;
        let static_class = |name: &str| {
            self.call(
                self.select(self.local(&ctx.mirror_local), "staticClass"),
                vec![self.lit(Lit::String(name.to_string()))],
            )
        };
        match body {
            TagBody::StaticClass(name) => self.select(
                self.select(static_class(name), "asType"),
                "toTypeConstructor",
            ),
            TagBody::Applied { class_name, args } => {
                let list = self.call(
                    self.node(TreeKind::Ident {
                        name: "List".to_string(),
                    }),
                    args.iter().map(|a| self.rebuild_type(ctx, a)).collect(),
                );
                self.call(
                    self.universe_member("appliedType"),
                    vec![static_class(class_name), list],
                )
            }
            TagBody::FromTag(tag) => self.select(self.rebased(ctx, tag), "tpe"),
        }
    }

    /// `<e>.in[$u.type]($m)` -- an `Expr` or a tag moved into the mirror the
    /// creator was handed.
    ///
    /// The type argument is written out because `$m`'s own type is
    /// `Mirror[$u.type]` only after the cast the creator makes: the universe's
    /// abstract `Mirror` loses its `api.Mirror[self.type]` bound in the
    /// pickle (`docs/macros.md` §7.14, item 3).
    fn rebased(&self, ctx: &ReifyCtx, e: &Tree) -> Tree {
        let singleton = self.node(TreeKind::SingletonTypeTree {
            ref_: Box::new(self.local(&ctx.universe_local)),
        });
        self.call(
            self.node(TreeKind::TypeApply {
                fun: Box::new(self.select(e.clone(), "in")),
                args: vec![singleton],
            }),
            vec![self.local(&ctx.mirror_local)],
        )
    }

    /// `<e>.in[$u.type]($m).tree` -- what `.splice` becomes.
    ///
    /// `in` rebases the `Expr` into the mirror the creator was handed, so the
    /// spliced tree belongs to the same universe as the one being built
    /// around it. The type argument is written out because `$m`'s own type is
    /// `Mirror[$u.type]` only after the cast the creator makes: the universe's
    /// abstract `Mirror` loses its `api.Mirror[self.type]` bound in the
    /// pickle (`docs/macros.md` §7.14, item 3).
    fn splice_tree(&self, ctx: &ReifyCtx, e: &Tree) -> Tree {
        self.select(self.rebased(ctx, e), "tree")
    }

    /// `new C`, `new C(a)`, `new C(a)(b)` -- `Ok(None)` when `t` is not one.
    ///
    /// nsc reifies the whole spine into a single
    /// `SyntacticNew(Nil, List(<parent>), noSelfType, Nil)`, with the argument
    /// clauses wrapped around the parent type by `SyntacticApplied`. Recursing
    /// through `Apply` instead would put the `SyntacticApplied` *outside* the
    /// `SyntacticNew`, which is a different tree.
    fn new_spine(&self, t: &Tree) -> Result<Option<Tree>, String> {
        // `new C { ... }` is an anonymous class after parsing; nsc keeps the
        // parents and the body in the `SyntacticNew` itself.
        if let Some(t) = self.anon_new(t)? {
            return Ok(Some(t));
        }
        let mut clauses: Vec<&Vec<Tree>> = Vec::new();
        let mut cur = t;
        loop {
            match &cur.kind {
                TreeKind::Apply { fun, args } => {
                    clauses.push(args);
                    cur = fun;
                }
                TreeKind::New { tpt } => {
                    clauses.reverse();
                    // The parser folds a constructor's *first* argument clause
                    // inside the `New` itself (`new C(1)(2)` is
                    // `Apply(New(Apply(C, 1)), 2)`), so peel those off first
                    // or the clauses come out in the wrong order.
                    let mut inner: Vec<&Vec<Tree>> = Vec::new();
                    let mut head = tpt.as_ref();
                    while let TreeKind::Apply { fun, args } = &head.kind {
                        inner.push(args);
                        head = fun;
                    }
                    inner.reverse();
                    inner.extend(clauses);
                    let clauses = inner;
                    let mut parent = self.typ(head)?;
                    if !clauses.is_empty() {
                        let mut cs = Vec::new();
                        for c in clauses {
                            cs.push(self.arg_clause(c)?);
                        }
                        parent = self.call(
                            self.support_member("SyntacticApplied"),
                            vec![parent, self.list(cs)],
                        );
                    }
                    return Ok(Some(self.call(
                        self.support_member("SyntacticNew"),
                        vec![
                            self.list(vec![]),
                            self.list(vec![parent]),
                            self.universe_member("noSelfType"),
                            self.list(vec![]),
                        ],
                    )));
                }
                _ => return Ok(None),
            }
        }
    }

    /// A function literal. `{ case ... }` is a *partial* function, which the
    /// parser desugars to `x$pf => x$pf match { ... }`; nsc keeps it as
    /// `SyntacticPartialFunction`, so the desugaring is undone here.
    fn function(&self, vparams: &[Tree], body: &Tree) -> Result<Tree, String> {
        if let (1, TreeKind::Match { selector, cases }) = (vparams.len(), &body.kind) {
            let synthetic = matches!(&vparams[0].kind,
                TreeKind::ValDef { name, tpt, rhs, .. }
                    if name == "x$pf"
                        && matches!(tpt.kind, TreeKind::Empty)
                        && matches!(rhs.kind, TreeKind::Empty))
                && matches!(&selector.kind, TreeKind::Ident { name } if name == "x$pf");
            if synthetic {
                let cs = self.case_defs(cases)?;
                return Ok(self.call(
                    self.support_member("SyntacticPartialFunction"),
                    vec![self.list(cs)],
                ));
            }
        }
        let mut ps = Vec::new();
        let scope = self.fresh.borrow().params.len();
        for p in vparams {
            let TreeKind::ValDef {
                mods, name, tpt, ..
            } = &p.kind
            else {
                return Err("a function literal's parameter is not reified yet".to_string());
            };
            // The parser turns `_.get` into a lambda over a parameter it
            // invented (`x$1`, `PARAM | SYNTHETIC`). nsc invents one too, but
            // draws its name from the universe at run time, so the name is
            // `freshTermName("x$")` and every occurrence in the body is that
            // same local -- not a `TermName` built from what the parser chose.
            let (flags, name) = if is_parser_placeholder(mods.flags, name) {
                let local = self.fresh_name(true, "x$");
                self.fresh
                    .borrow_mut()
                    .params
                    .push((name.clone(), local.clone()));
                (PLACEHOLDER_PARAM_FLAGS, self.local(&local))
            } else if mods.flags == Flags::PARAM {
                (PARAM_FLAGS, self.term_name_or_hole(name)?)
            } else {
                return Err("a modified function literal parameter is not reified yet".to_string());
            };
            let mods = self.mods(flags);
            ps.push(self.call(
                self.support_member("SyntacticValDef"),
                vec![
                    mods,
                    name,
                    self.type_or_empty(tpt)?,
                    self.universe_member("EmptyTree"),
                ],
            ));
        }
        let b = self.term(body);
        self.fresh.borrow_mut().params.truncate(scope);
        Ok(self.call(
            self.support_member("SyntacticFunction"),
            vec![self.list(ps), b?],
        ))
    }

    /// One statement of a block: a definition, or an ordinary term.
    /// `crates/typer/src/reify_defs.rs` holds the definitions.
    fn stat(&self, t: &Tree) -> Result<Tree, String> {
        self.definition(t)
    }

    // -- types -------------------------------------------------------------

    /// Lower one type of the body: the whole of `tq"..."`, and the right-hand
    /// side of an ascription or a type application inside `q"..."`.
    fn typ(&self, t: &Tree) -> Result<Tree, String> {
        // A type in a reified body has to be rebuilt from its symbol too, and
        // that is a second reifier (nsc's `reifyType`) scala-rs does not have.
        // Refused rather than reified as the written name, which would mean
        // whatever the call site's scope makes of it.
        if let Some(ctx) = &self.reify {
            return match ctx.refs.get(&t.id) {
                Some(ReifyRef::Type(body)) => Ok(self.call(
                    self.support_member("mkTypeTree"),
                    vec![self.rebuild_type(ctx, body)],
                )),
                Some(ReifyRef::TypeGap(why)) => {
                    Err(format!("a type argument cannot be rebuilt: {why}"))
                }
                _ => Err(format!(
                    "{} in a `reify` body is not reified yet",
                    describe_type(&t.kind)
                )),
            };
        }
        match &t.kind {
            // Written with the `apply` spelled out: `SyntacticEmptyTypeTree`
            // is a parameterless `def` returning the extractor, and an empty
            // argument list on such a def is the def itself, not a call of the
            // extractor.
            TreeKind::Empty => Ok(self.call(
                self.select(self.support_member("SyntacticEmptyTypeTree"), "apply"),
                vec![],
            )),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0, Pos::Type),
                None => Ok(self.call(
                    self.support_member("SyntacticTypeIdent"),
                    vec![self.type_name(name)],
                )),
            },
            // `a.b.C`: the prefix is a *term*, the last name a type.
            TreeKind::Select { qual, name } => {
                let q = self.term(qual)?;
                Ok(self.call(
                    self.support_member("SyntacticSelectType"),
                    vec![q, self.type_name_or_hole(name)?],
                ))
            }
            TreeKind::SelectFromTypeTree { qual, name, hash } => {
                if !*hash {
                    return Err("a path-dependent type is not reified yet".to_string());
                }
                let q = self.typ(qual)?;
                Ok(self.call(
                    self.support_member("SyntacticTypeProjection"),
                    vec![q, self.type_name_or_hole(name)?],
                ))
            }
            TreeKind::SingletonTypeTree { ref_ } => {
                let r = self.term(ref_)?;
                Ok(self.call(self.support_member("SyntacticSingletonType"), vec![r]))
            }
            TreeKind::AppliedTypeTree { tpt, args } => self.applied_type(tpt, args),
            TreeKind::CompoundTypeTree {
                parents,
                refinements,
            } => {
                if !refinements.is_empty() {
                    return Err("a refinement type is not reified yet".to_string());
                }
                let mut ps = Vec::new();
                for p in parents {
                    ps.push(self.typ(p)?);
                }
                Ok(self.call(
                    self.support_member("SyntacticCompoundType"),
                    vec![self.list(ps), self.list(vec![])],
                ))
            }
            // A `_` type argument the enclosing application has bound: the
            // fresh `TypeName` its `SyntacticExistentialType` introduces. One
            // that reached here unbound (`tq"_"`, which nsc rejects too) falls
            // through to the diagnostic below.
            TreeKind::TypeDef { .. } if is_wildcard_type(t) => {
                // A bare `_` in a pattern is a *type-variable pattern*, which
                // nsc writes `Bind(TypeName("_"), EmptyTree)` and gives no
                // name; only a bounded one is an existential there.
                if !self.wildcard_binds_a_name(t) {
                    return Ok(self.call(
                        self.universe_member("Bind"),
                        vec![self.type_name("_"), self.universe_member("EmptyTree")],
                    ));
                }
                match self.wildcard_local(t.id) {
                    Some(local) => Ok(self.call(
                        self.support_member("SyntacticTypeIdent"),
                        vec![self.local(&local)],
                    )),
                    None => Err(format!("{} is not reified yet", describe_type(&t.kind))),
                }
            }
            other => Err(format!("{} is not reified yet", describe_type(other))),
        }
    }

    /// `F[A, B]`, `(A, B)`, and `A => B` -- three different nsc calls, and the
    /// parser folds the last two into `AppliedTypeTree` of a synthesised head
    /// (`Ident("<tuple>")`, `Ident("FunctionN")`). The tuple marker cannot be
    /// written, but `Function1` can, so the arrow is told apart by the source
    /// text under the head's span, which is the operand or the `=>` when the
    /// head was synthesised.
    fn applied_type(&self, tpt: &Tree, args: &[Tree]) -> Result<Tree, String> {
        // `P[_, _]` is an existential: nsc names each `_` with
        // `freshTypeName("_$")` and wraps *this* application -- the innermost
        // one whose own arguments hold the wildcards -- in
        // `SyntacticExistentialType` over a `TypeDef` per name.
        let wild: Vec<&Tree> = args
            .iter()
            .filter(|a| is_wildcard_type(a) && self.wildcard_binds_a_name(a))
            .collect();
        if !wild.is_empty() {
            let scope = self.fresh.borrow().wilds.len();
            let mut defs = Vec::new();
            for w in wild {
                let local = self.fresh_name(false, "_$");
                defs.push(self.wildcard_type_def(w, &local)?);
                self.fresh.borrow_mut().wilds.push((w.id, local));
            }
            let inner = self.applied_type_head(tpt, args);
            self.fresh.borrow_mut().wilds.truncate(scope);
            return Ok(self.call(
                self.support_member("SyntacticExistentialType"),
                vec![inner?, self.list(defs)],
            ));
        }
        self.applied_type_head(tpt, args)
    }

    /// Whether this `_` type argument is an existential, which binds a fresh
    /// `TypeName`, rather than a *type-variable pattern*.
    ///
    /// They are the same syntax and only the position tells them apart: inside
    /// a pattern a bare `_` matches any type argument and nsc writes it
    /// `Bind(TypeName("_"), EmptyTree)`, while one carrying bounds
    /// (`pq"_: R[_ <: Int]"`) is an existential there as everywhere else.
    fn wildcard_binds_a_name(&self, w: &Tree) -> bool {
        let TreeKind::TypeDef { lo, hi, .. } = &w.kind else {
            return false;
        };
        self.fresh.borrow().pat_depth == 0 || lo.is_some() || hi.is_some()
    }

    /// `u.TypeDef(u.Modifiers(DEFERRED | SYNTHETIC), <local>, Nil,
    /// u.TypeBoundsTree(<lo>, <hi>))` for one `_` type argument.
    fn wildcard_type_def(&self, w: &Tree, local: &str) -> Result<Tree, String> {
        let TreeKind::TypeDef { lo, hi, .. } = &w.kind else {
            unreachable!("only a wildcard type gets here");
        };
        let bound = |b: &Option<Box<Tree>>| match b {
            Some(t) => self.typ(t),
            None => Ok(self.universe_member("EmptyTree")),
        };
        let bounds = self.call(
            self.universe_member("TypeBoundsTree"),
            vec![bound(lo)?, bound(hi)?],
        );
        Ok(self.call(
            self.universe_member("TypeDef"),
            vec![
                self.mods(EXISTENTIAL_TPARAM_FLAGS),
                self.local(local),
                self.list(vec![]),
                bounds,
            ],
        ))
    }

    /// `applied_type` once the wildcards among `args`, if any, are bound.
    fn applied_type_head(&self, tpt: &Tree, args: &[Tree]) -> Result<Tree, String> {
        if let TreeKind::Ident { name } = &tpt.kind {
            if name == "<tuple>" {
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.typ(a)?);
                }
                return Ok(self.call(
                    self.support_member("SyntacticTupleType"),
                    vec![self.list(ts)],
                ));
            }
            if let Some(rest) = name.strip_prefix("Function") {
                let written = self.text(tpt.span) == name.as_str();
                if !written && rest.chars().all(|c| c.is_ascii_digit()) && !args.is_empty() {
                    // `=> T` (a by-name type) also lands here; nsc's own
                    // parser rejects it inside `tq"..."`, so it is refused
                    // rather than turned into `() => T`.
                    if self.text(tpt.span).starts_with("=>") {
                        return Err("a by-name type is not reified yet".to_string());
                    }
                    let (params, res) = args.split_at(args.len() - 1);
                    let mut ps = Vec::new();
                    for p in params {
                        ps.push(self.typ(p)?);
                    }
                    let r = self.typ(&res[0])?;
                    return Ok(self.call(
                        self.support_member("SyntacticFunctionType"),
                        vec![self.list(ps), r],
                    ));
                }
            }
        }
        let head = self.typ(tpt)?;
        let mut ts = Vec::new();
        for a in args {
            ts.push(self.typ(a)?);
        }
        Ok(self.call(
            self.support_member("SyntacticAppliedType"),
            vec![head, self.list(ts)],
        ))
    }

    /// A `val`'s or parameter's type, `SyntacticEmptyTypeTree()` when absent.
    fn type_or_empty(&self, tpt: &Tree) -> Result<Tree, String> {
        self.typ(tpt)
    }

    // -- patterns ----------------------------------------------------------

    /// Lower one pattern: the whole of `pq"..."`, and a `case` clause's
    /// pattern inside `q"..."` / `cq"..."`.
    ///
    /// The depth is what tells the type walk it is under a pattern, where a
    /// `_` type argument is a `Bind` and not an existential.
    fn pat(&self, t: &Tree) -> Result<Tree, String> {
        self.fresh.borrow_mut().pat_depth += 1;
        let r = self.pat_inner(t);
        self.fresh.borrow_mut().pat_depth -= 1;
        r
    }

    fn pat_inner(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Literal { lit } => Ok(self.constant(lit.clone())),
            TreeKind::Wildcard => Ok(self.term_ident("_")),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0, Pos::Pat),
                None if name == "_" => Ok(self.term_ident("_")),
                // Scala's rule: an identifier that starts lower case is a
                // variable pattern, which nsc reifies as a `Bind` over `_`;
                // anything else is a stable identifier.
                None if starts_lower(name) => Ok(self.call(
                    self.universe_member("Bind"),
                    vec![self.term_name(name), self.term_ident("_")],
                )),
                None => Ok(self.term_ident(name)),
            },
            // A stable-identifier pattern (`a.b.None`): a term selection.
            TreeKind::Select { qual, name } => {
                let q = self.term(qual)?;
                self.select_term(q, name, t.span)
            }
            TreeKind::Bind { name, body } => {
                let b = self.pat(body)?;
                Ok(self.call(
                    self.universe_member("Bind"),
                    vec![self.term_name_or_hole(name)?, b],
                ))
            }
            TreeKind::Alternative { trees } => {
                let mut ps = Vec::new();
                for p in trees {
                    ps.push(self.pat(p)?);
                }
                Ok(self.call(self.universe_member("Alternative"), vec![self.list(ps)]))
            }
            TreeKind::Typed { expr, tpt } => {
                let e = self.pat(expr)?;
                let ty = self.typ(tpt)?;
                Ok(self.call(self.universe_member("Typed"), vec![e, ty]))
            }
            // `Foo(a, b)` / `Foo[T](a)`: an extractor, reified exactly like an
            // application whose arguments happen to be patterns.
            TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
                let f = self.pat_callee(fun)?;
                let clause = self.pat_clause(args)?;
                Ok(self.call(
                    self.support_member("SyntacticApplied"),
                    vec![f, self.list(vec![clause])],
                ))
            }
            other => Err(format!("{} is not reified yet", describe(other))),
        }
    }

    /// The extractor of a pattern application. It is a term, and never a
    /// variable pattern however it is spelled.
    fn pat_callee(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::TypeApply { fun, args } => {
                let f = self.pat_callee(fun)?;
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.typ(a)?);
                }
                Ok(self.call(
                    self.support_member("SyntacticTypeApplied"),
                    vec![f, self.list(ts)],
                ))
            }
            _ => self.term(t),
        }
    }

    /// The arguments of a pattern application, as a `List[Tree]`.
    fn pat_clause(&self, args: &[Tree]) -> Result<Tree, String> {
        if let Some(t) = self.splice_clause(args, Pos::Pat, &|a| self.pat(a))? {
            return Ok(t);
        }
        let mut out = Vec::new();
        for a in args {
            out.push(self.pat(a)?);
        }
        Ok(self.list(out))
    }

    // -- case clauses ------------------------------------------------------

    fn case_defs(&self, cases: &[CaseDef]) -> Result<Vec<Tree>, String> {
        let mut out = Vec::new();
        for c in cases {
            out.push(self.case_def(c)?);
        }
        Ok(out)
    }

    fn case_def(&self, c: &CaseDef) -> Result<Tree, String> {
        let p = self.pat(&c.pat)?;
        let guard = if matches!(&c.guard.kind, TreeKind::Empty) {
            self.universe_member("EmptyTree")
        } else {
            self.term(&c.guard)?
        };
        let body = self.term(&c.body)?;
        Ok(self.call(self.universe_member("CaseDef"), vec![p, guard, body]))
    }

    // -- shared pieces -----------------------------------------------------

    /// One parameter clause as a `List[Tree]`.
    fn arg_clause(&self, args: &[Tree]) -> Result<Tree, String> {
        if let Some(t) = self.splice_clause(args, Pos::Term, &|a| self.term(a))? {
            return Ok(t);
        }
        let mut out = Vec::new();
        for a in args {
            out.push(self.term(a)?);
        }
        Ok(self.list(out))
    }

    /// `rs.SyntacticBlock(<stats>)` when `elems` contains a `..$xs`.
    fn stats_splice(&self, elems: &[Tree]) -> Result<Option<Tree>, String> {
        Ok(self
            .splice_clause(elems, Pos::Term, &|a| self.stat(a))?
            .map(|xs| self.call(self.support_member("SyntacticBlock"), vec![xs])))
    }

    /// A clause containing at least one `..$xs`, as a `List[Tree]`; `None`
    /// when there is no splice in it and the caller's plain `List(...)` is the
    /// answer.
    ///
    /// nsc's `reifyList`: runs of ordinary elements become one `List(...)`
    /// each, every rank-1 hole stands for itself, and the pieces are joined
    /// left to right with `++`. So `f(a, ..$xs, b)` reifies as
    /// `List(<a>) ++ xs ++ List(<b>)` -- the order of the call's arguments is
    /// the order of the concatenation, and each piece is already a
    /// `List[Tree]`, so no piece's static type has to be guessed at.
    ///
    /// `plain` lowers an ordinary element, and differs by clause: an argument
    /// is a term, a pattern argument a pattern, a block element a statement.
    fn splice_clause(
        &self,
        args: &[Tree],
        pos: Pos,
        plain: &dyn Fn(&Tree) -> Result<Tree, String>,
    ) -> Result<Option<Tree>, String> {
        let rank = |a: &Tree| match &a.kind {
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.ranks.get(i).copied().unwrap_or(0),
                None => 0,
            },
            _ => 0,
        };
        if args.iter().all(|a| rank(a) == 0) {
            return Ok(None);
        }
        let mut parts: Vec<Tree> = Vec::new();
        let mut run: Vec<Tree> = Vec::new();
        for a in args {
            if rank(a) == 0 {
                run.push(plain(a)?);
                continue;
            }
            if !run.is_empty() {
                parts.push(self.list(std::mem::take(&mut run)));
            }
            let TreeKind::Ident { name } = &a.kind else {
                unreachable!("a non-zero rank comes from a hole");
            };
            let i = hole_index(name).expect("a non-zero rank comes from a hole");
            // Rank 2 and up (`...$xss`) is still refused, by `hole` itself.
            parts.push(self.hole(i, 1, pos)?);
        }
        if !run.is_empty() {
            parts.push(self.list(run));
        }
        let mut parts = parts.into_iter();
        let mut out = parts.next().expect("there is at least one splice");
        for p in parts {
            out = self.call(self.select(out, "++"), vec![p]);
        }
        Ok(Some(out))
    }

    /// The expression filling hole `i`, which must have been written at
    /// `rank`, lifted for the position it stands in.
    fn hole(&self, i: usize, rank: u8, pos: Pos) -> Result<Tree, String> {
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
        let arg = self
            .args
            .get(i)
            .cloned()
            .ok_or_else(|| format!("hole {i} has no argument"))?;
        let lift = self.lifts.get(i).unwrap_or(&Lift::Tree);
        self.lift(arg, lift, pos)
    }

    /// Build the tree the standard `Liftable` for `lift` builds around `arg`.
    ///
    /// A `Name` is the one that depends on where it stands: nsc's quasiquote
    /// parser puts the hole in an identifier position, so `q"$n"` is a term
    /// identifier, `tq"$n"` a type identifier and `pq"$n"` a variable pattern.
    fn lift(&self, arg: Tree, lift: &Lift, pos: Pos) -> Result<Tree, String> {
        match lift {
            Lift::Tree => Ok(arg),
            Lift::Name => Ok(match pos {
                Pos::Name => arg,
                Pos::Term => self.call(
                    self.support_member("SyntacticTermIdent"),
                    vec![arg, self.lit(Lit::Boolean(false))],
                ),
                Pos::Type => self.call(self.support_member("SyntacticTypeIdent"), vec![arg]),
                Pos::Pat => self.call(
                    self.universe_member("Bind"),
                    vec![arg, self.term_ident("_")],
                ),
            }),
            // `liftInt` &co: `Liftable(v => Literal(Constant(v)))`.
            Lift::Value => Ok(self.call(
                self.universe_member("Literal"),
                vec![self.call(self.universe_member("Constant"), vec![arg])],
            )),
            Lift::Constant => Ok(self.call(self.universe_member("Literal"), vec![arg])),
            // `liftType` / `liftTypeTag`: `TypeTree(tpe)`, which the reflect
            // API spells `internal.reificationSupport.mkTypeTree`.
            Lift::Type => Ok(self.call(self.support_member("mkTypeTree"), vec![arg])),
            Lift::TypeTag => Ok(self.call(
                self.support_member("mkTypeTree"),
                vec![self.select(arg, "tpe")],
            )),
            Lift::Expr => Ok(self.select(arg, "tree")),
            Lift::Symbol => Ok(self.call(
                self.support_member("mkRefTree"),
                vec![self.universe_member("EmptyTree"), arg],
            )),
            Lift::Elems { to_list, elem } => {
                let xs = if *to_list {
                    self.select(arg, "toList")
                } else {
                    arg
                };
                if **elem == Lift::Tree {
                    return Ok(xs);
                }
                // `xs.map(v => <lift v>)`, the shape nsc writes with a
                // `freshTermName`. The parameter is only ever read by the
                // lift, so the name cannot collide with the body's.
                let name = "x$qq";
                let param = self.node(TreeKind::ValDef {
                    mods: Modifiers::new(Flags::PARAM),
                    name: name.to_string(),
                    tpt: Box::new(self.node(TreeKind::Empty)),
                    rhs: Box::new(self.node(TreeKind::Empty)),
                });
                let v = self.node(TreeKind::Ident {
                    name: name.to_string(),
                });
                let body = self.lift(v, elem, pos)?;
                let f = self.node(TreeKind::Function {
                    vparams: vec![param],
                    body: Box::new(body),
                });
                Ok(self.call(self.select(xs, "map"), vec![f]))
            }
            Lift::Unknown(ty) => Err(format!(
                "a hole of type `{ty}` is not lifted (the Liftable instances \
                 scala-rs builds are trees, names, symbols, types, type tags, \
                 `Expr`s, constants and literals)"
            )),
        }
    }

    // -- fresh names -------------------------------------------------------

    /// Bind `rs.freshTermName("<prefix>")` (or `freshTypeName`) to a new local
    /// and answer that local's name.
    ///
    /// The local is `nn$macro$k`, nsc's own spelling. Its scope is the block
    /// `reify` puts around this quasiquote, so two quasiquotes -- including a
    /// nested one spliced through a hole -- never see each other's.
    fn fresh_name(&self, term: bool, prefix: &str) -> String {
        let mut fresh = self.fresh.borrow_mut();
        fresh.next += 1;
        let local = format!("nn$macro${}", fresh.next);
        drop(fresh);
        let maker = if term {
            "freshTermName"
        } else {
            "freshTypeName"
        };
        let rhs = self.call(
            self.support_member(maker),
            vec![self.lit(Lit::String(prefix.to_string()))],
        );
        let def = self.node(TreeKind::ValDef {
            mods: Modifiers::default(),
            name: local.clone(),
            tpt: Box::new(self.node(TreeKind::Empty)),
            rhs: Box::new(rhs),
        });
        self.fresh.borrow_mut().defs.push(def);
        local
    }

    /// An `Ident` of a local `fresh_name` handed out.
    fn local(&self, name: &str) -> Tree {
        self.node(TreeKind::Ident {
            name: name.to_string(),
        })
    }

    /// The local holding the fresh name for the placeholder parameter the
    /// parser called `name`, if that is what `name` is.
    fn placeholder_param(&self, name: &str) -> Option<String> {
        let fresh = self.fresh.borrow();
        fresh
            .params
            .iter()
            .rev()
            .find(|(p, _)| p == name)
            .map(|(_, local)| local.clone())
    }

    /// The local holding the fresh name the existential binds for the `_`
    /// type argument `id`, if that argument is one being reified now.
    fn wildcard_local(&self, id: NodeId) -> Option<String> {
        let fresh = self.fresh.borrow();
        fresh
            .wilds
            .iter()
            .rev()
            .find(|(w, _)| *w == id)
            .map(|(_, local)| local.clone())
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
            scala_ref: false,
            stable_pat: false,
        }
    }

    fn lit(&self, lit: Lit) -> Tree {
        self.node(TreeKind::Literal { lit })
    }

    /// The text the body was parsed from, under `span`.
    fn text(&self, span: Span) -> &str {
        self.src
            .get(span.lo.to_usize()..span.hi.to_usize())
            .unwrap_or("")
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

    /// `<universe>.Literal(<universe>.Constant(v))`.
    fn constant(&self, lit: Lit) -> Tree {
        self.call(
            self.universe_member("Literal"),
            vec![self.call(self.universe_member("Constant"), vec![self.lit(lit)])],
        )
    }

    /// `<universe>.Modifiers(rs.FlagsRepr(<flags>L))`.
    ///
    /// `Modifiers(flags)` is `Modifiers(flags, typeNames.EMPTY, Nil)`, so
    /// `mods(0)` is the value `NoMods` names. It is written this way for two
    /// reasons: this alternative is an ordinary overload of `def Modifiers`
    /// and needs no `apply` insertion, and `NoMods` is declared on
    /// `scala.reflect.api.Universe`, an abstract *class* whose subtyping is
    /// recorded only in the pickle -- selecting it on a `JavaUniverse`
    /// receiver emitted an `invokevirtual` the JVM rejects.
    fn mods(&self, flags: i64) -> Tree {
        self.call(
            self.universe_member("Modifiers"),
            vec![self.call(
                self.support_member("FlagsRepr"),
                vec![self.lit(Lit::Long(flags))],
            )],
        )
    }

    /// `<universe>.TermName("<s>")`.
    ///
    /// Operator names are encoded the way nsc's parser encodes them, so
    /// `q"a.+(b)"` selects `$plus` and not `+`.
    fn term_name(&self, s: &str) -> Tree {
        self.call(
            self.universe_member("TermName"),
            vec![self.lit(Lit::String(encode_method_name(s)))],
        )
    }

    /// `<universe>.TypeName("<s>")`.
    fn type_name(&self, s: &str) -> Tree {
        self.call(
            self.universe_member("TypeName"),
            vec![self.lit(Lit::String(encode_method_name(s)))],
        )
    }

    /// A name position that may itself be a hole: `q"$x.$n"` splices the
    /// `TermName` straight in.
    fn term_name_or_hole(&self, s: &str) -> Result<Tree, String> {
        match hole_index(s) {
            Some(i) => self.hole(i, 0, Pos::Name),
            None => Ok(self.term_name(s)),
        }
    }

    fn type_name_or_hole(&self, s: &str) -> Result<Tree, String> {
        match hole_index(s) {
            Some(i) => self.hole(i, 0, Pos::Name),
            None => Ok(self.type_name(s)),
        }
    }

    /// `rs.SyntacticTermIdent(u.TermName("<s>"), false)`.
    fn term_ident(&self, s: &str) -> Tree {
        self.call(
            self.support_member("SyntacticTermIdent"),
            vec![self.term_name(s), self.lit(Lit::Boolean(false))],
        )
    }

    /// `rs.SyntacticSelectTerm(<qual>, u.TermName("<name>"))`.
    ///
    /// An *infix* right-associative operator never gets here: the parser has
    /// turned `a :: b` into `b.::(a)`, and nsc builds neither that nor the
    /// plain selection but a block binding the left operand first, which
    /// `right_assoc` builds. A written `b.::(a)` is an ordinary selection and
    /// does get here; the two are told apart by the source text, since the
    /// operator comes *before* its qualifier only when it was written infix.
    fn select_term(&self, qual: Tree, name: &str, span: Span) -> Result<Tree, String> {
        if self.written_infix(span, name) {
            return Err(format!(
                "a right-associative operator (`{name}`) in this position is not reified yet"
            ));
        }
        Ok(self.call(
            self.support_member("SyntacticSelectTerm"),
            vec![qual, self.term_name_or_hole(name)?],
        ))
    }

    /// Whether the selection under `span` is a right-associative operator that
    /// was written infix (`a :: b`), rather than as a call (`b.::(a)`).
    ///
    /// The parser puts the *right* operand in the qualifier, so the merged
    /// span of the selection starts at the operator; a written selection
    /// starts at its qualifier.
    fn written_infix(&self, span: Span, name: &str) -> bool {
        is_right_associative(name) && self.text(span).starts_with(name)
    }

    /// `a :: b` -- `Ok(None)` when the application is not one.
    ///
    /// nsc keeps Scala's evaluation order (left operand first) by binding it
    /// to a fresh `val` and calling the operator on that:
    ///
    /// ```text
    /// { val rassoc$1 = a; b.::(rassoc$1) }
    /// ```
    ///
    /// so the reified tree is a `SyntacticBlock`, not a bare application.
    fn right_assoc(&self, fun: &Tree, args: &[Tree]) -> Result<Option<Tree>, String> {
        let TreeKind::Select { qual, name } = &fun.kind else {
            return Ok(None);
        };
        if !self.written_infix(fun.span, name) {
            return Ok(None);
        }
        let [lhs] = args else {
            return Ok(None);
        };
        let local = self.fresh_name(true, "rassoc$");
        let bound = self.call(
            self.support_member("SyntacticValDef"),
            vec![
                self.mods(RASSOC_VAL_FLAGS),
                self.local(&local),
                self.type_or_empty(&self.node(TreeKind::Empty))?,
                self.term(lhs)?,
            ],
        );
        let recv = self.term(qual)?;
        let sel = self.call(
            self.support_member("SyntacticSelectTerm"),
            vec![recv, self.term_name(name)],
        );
        let arg = self.call(
            self.support_member("SyntacticTermIdent"),
            vec![self.local(&local), self.lit(Lit::Boolean(false))],
        );
        let applied = self.call(
            self.support_member("SyntacticApplied"),
            vec![sel, self.list(vec![self.list(vec![arg])])],
        );
        Ok(Some(self.call(
            self.support_member("SyntacticBlock"),
            vec![self.list(vec![bound, applied])],
        )))
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

/// Whether a name is a variable pattern rather than a stable identifier.
fn starts_lower(name: &str) -> bool {
    match name.chars().next() {
        Some(c) => c == '_' || c.is_lowercase(),
        None => false,
    }
}

/// Whether `name` is the `TupleN` the parser synthesises for `(a, ..., z)`
/// with that many elements.
fn is_tuple_name(name: &str, arity: usize) -> bool {
    name.strip_prefix("Tuple")
        .and_then(|n| n.parse::<usize>().ok())
        == Some(arity)
}

/// Scala's rule: a method whose name ends in `:` is right-associative.
fn is_right_associative(name: &str) -> bool {
    name.len() > 1 && name.ends_with(':') && !name.ends_with("::=")
}

/// Whether `flags` and `name` are what `Parser::fresh_placeholder` puts on the
/// parameter it invents for a `_` in expression position (`_.get`), as opposed
/// to a parameter the source wrote.
fn is_parser_placeholder(flags: Flags, name: &str) -> bool {
    flags == Flags::PARAM.with(Flags::SYNTHETIC)
        && name
            .strip_prefix("x$")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `t` is the anonymous `TypeDef` the parser makes for a `_` (or `?`)
/// type argument.
fn is_wildcard_type(t: &Tree) -> bool {
    matches!(&t.kind, TreeKind::TypeDef { name, tparams, rhs, .. }
        if name == "_" && tparams.is_empty() && matches!(rhs.kind, TreeKind::Empty))
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
        TreeKind::Try { .. } => "`try`",
        TreeKind::Throw { .. } => "`throw`",
        TreeKind::Return { .. } => "`return`",
        TreeKind::While { .. } | TreeKind::DoWhile { .. } => "`while`",
        TreeKind::Star { .. } => "a `_*` pattern",
        TreeKind::ValDef { .. } => "a `val` definition",
        TreeKind::DefDef { .. } => "a `def` definition",
        TreeKind::ClassDef { .. } => "a class definition",
        TreeKind::ModuleDef { .. } => "an object definition",
        TreeKind::TypeDef { .. } => "a type definition",
        TreeKind::InterpolatedString { .. } => "a nested interpolation",
        _ => "this form",
    }
}

/// The same, for a form met in type position.
fn describe_type(k: &TreeKind) -> &'static str {
    match k {
        TreeKind::ExistentialTypeTree { .. } => "an existential type",
        TreeKind::AnnotatedTypeTree { .. } => "an annotated type",
        // The parser turns a `_` type argument into the existential's bound
        // type parameter. nsc names those with `freshTypeName` and binds them
        // in a block around the call, so the names -- and the trees -- differ.
        TreeKind::TypeDef { .. } => "a `_` type argument (an existential)",
        TreeKind::Wildcard => "a wildcard type",
        TreeKind::Function { .. } => "a by-name type",
        _ => describe(k),
    }
}
