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

use scala_rs_parser::{CaseDef, Lit, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_pickle::names::encode_method_name;
use scala_rs_span::Span;

use crate::quasiquote::{hole_index, QuasiKind};
use crate::uncurry::is_eta_marker;

/// Definitions -- `class`, `trait`, `object`, `def`, `val` -- live in their
/// own file. It is a child module rather than a sibling so it can use the
/// building blocks below without any of them becoming visible crate-wide.
#[path = "reify_defs.rs"]
mod defs;

/// The `Modifiers` flag set nsc gives a function literal's parameter
/// (`Flags.PARAM`, `1 << 13`). Read off `-Ymacro-debug-lite` for
/// `q"(y: Int) => y"`, which reifies the parameter as
/// `SyntacticValDef(Modifiers(FlagsRepr(8192L), TypeName(""), List()), ...)`.
const PARAM_FLAGS: i64 = 8192;

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
}

impl<'a> Reifier<'a> {
    pub(crate) fn new(
        universe: Tree,
        args: &'a [Tree],
        ranks: &'a [u8],
        span: Span,
        src: &'a str,
    ) -> Self {
        Reifier {
            universe,
            args,
            ranks,
            span,
            src,
        }
    }

    /// Lower a quasiquote body.
    pub(crate) fn reify(&self, kind: QuasiKind, body: &Tree) -> Result<Tree, String> {
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
        // `new C(a)(b)`: the parser leaves an application spine whose head is
        // `New`, and nsc puts *every* clause inside the one `SyntacticNew`.
        if let Some(t) = self.new_spine(t)? {
            return Ok(t);
        }
        match &t.kind {
            TreeKind::Literal { lit } => Ok(self.constant(lit.clone())),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0),
                None => Ok(self.term_ident(name)),
            },
            TreeKind::Select { qual, name } => {
                // `$x.foo` and `a.b`: the qualifier is a term either way.
                let q = self.term(qual)?;
                Ok(self.select_term(q, name)?)
            }
            TreeKind::Apply { fun, args } => {
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
        for p in vparams {
            let TreeKind::ValDef {
                mods, name, tpt, ..
            } = &p.kind
            else {
                return Err("a function literal's parameter is not reified yet".to_string());
            };
            // The parser turns `_.get` into a lambda over a parameter it
            // invented; nsc keeps the placeholder and reifies it with a
            // `freshTermName` bound in a block around the call, so the
            // parameter's *name* differs and the trees are not the same.
            if mods.flags != scala_rs_parser::Flags::PARAM {
                return Err("a `_` placeholder function literal is not reified yet".to_string());
            }
            let mods = self.mods(PARAM_FLAGS);
            ps.push(self.call(
                self.support_member("SyntacticValDef"),
                vec![
                    mods,
                    self.term_name_or_hole(name)?,
                    self.type_or_empty(tpt)?,
                    self.universe_member("EmptyTree"),
                ],
            ));
        }
        let b = self.term(body)?;
        Ok(self.call(
            self.support_member("SyntacticFunction"),
            vec![self.list(ps), b],
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
                Some(i) => self.hole(i, 0),
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
    fn pat(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Literal { lit } => Ok(self.constant(lit.clone())),
            TreeKind::Wildcard => Ok(self.term_ident("_")),
            TreeKind::Ident { name } => match hole_index(name) {
                Some(i) => self.hole(i, 0),
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
                self.select_term(q, name)
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
        if let Some(t) = self.splice_clause(args)? {
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
    ///
    /// Two shapes are built: every argument an ordinary term, or a single
    /// `..$xs` standing for the whole clause. Mixing the two (`f(a, ..$xs)`)
    /// needs a concatenation whose static type has to be right on both sides,
    /// and building it wrong would silently reorder a call's arguments, so it
    /// is refused instead.
    fn arg_clause(&self, args: &[Tree]) -> Result<Tree, String> {
        if let Some(t) = self.splice_clause(args)? {
            return Ok(t);
        }
        let mut out = Vec::new();
        for a in args {
            out.push(self.term(a)?);
        }
        Ok(self.list(out))
    }

    /// `rs.SyntacticBlock(<xs>)` when `elems` is exactly one `..$xs`.
    fn stats_splice(&self, elems: &[Tree]) -> Result<Option<Tree>, String> {
        Ok(self
            .splice_clause(elems)?
            .map(|xs| self.call(self.support_member("SyntacticBlock"), vec![xs])))
    }

    /// `..$xs` standing for a whole clause, if that is what `args` is.
    fn splice_clause(&self, args: &[Tree]) -> Result<Option<Tree>, String> {
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
        if args.len() == 1 && rank(&args[0]) == 1 {
            let TreeKind::Ident { name } = &args[0].kind else {
                unreachable!("rank comes from a hole");
            };
            let i = hole_index(name).expect("rank comes from a hole");
            return self.hole(i, 1).map(Some);
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
            Some(i) => self.hole(i, 0),
            None => Ok(self.term_name(s)),
        }
    }

    fn type_name_or_hole(&self, s: &str) -> Result<Tree, String> {
        match hole_index(s) {
            Some(i) => self.hole(i, 0),
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
    /// A right-associative operator is refused: the parser has already turned
    /// `a :: b` into `b.::(a)`, which is indistinguishable from a written
    /// `b.::(a)` -- and nsc builds *neither* of those for the infix form (it
    /// binds the left operand to a fresh `val` first, to keep evaluation
    /// order). Guessing would silently swap a call's operands.
    fn select_term(&self, qual: Tree, name: &str) -> Result<Tree, String> {
        if is_right_associative(name) {
            return Err(format!(
                "a right-associative operator (`{name}`) is not reified yet"
            ));
        }
        Ok(self.call(
            self.support_member("SyntacticSelectTerm"),
            vec![qual, self.term_name_or_hole(name)?],
        ))
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
