//! `reify { … }` over the **typed** body: nsc's `scala.reflect.reify`
//! (`GenTrees` / `GenSymbols` / `GenTypes`) for scala-rs.
//!
//! A quasiquote lowers the *parsed* body by name, because a quasiquote's
//! meaning is settled where the tree it builds is finally typed. `reify` is
//! the opposite: the body was written in one scope and has to keep meaning
//! what it meant there wherever the expansion lands, so every reference is
//! rebuilt from the **symbol** it resolved to. The reifier therefore walks
//! the body *after* the typer has been over it (a clone typed once, in
//! `Check::try_expand_reify`), and the rules are nsc's own, measured with
//! `-Ymacro-debug-lite` (`docs/notes/reify-design.md`):
//!
//! * a definition **inside** the body, and every reference to it, is reified
//!   by name (`Ident(TermName("x"))`, `ValDef(...)`) -- the tree is untyped
//!   and whoever type-checks it later resolves the binding again;
//! * a static `object`, a member of one, a static class: through the mirror
//!   the creator is handed (`mkIdent($m.staticModule("..."))`,
//!   `Select(mkIdent(staticModule("scala.Predef")), TermName("println"))`,
//!   `mkIdent($m.staticClass("scala.Int"))`);
//! * a member of the `object` that lexically encloses the `reify`:
//!   `Select(mkThis($m.staticModule("M").asModule.moduleClass), name)`;
//! * a local or a parameter bound **outside** the body: a *free term*,
//!   `val free$x1 = newFreeTerm("x", x, FlagsRepr(...), "defined by f in
//!   F.scala:6:9")` followed by `setInfo(free$x1, <its type>)`, referenced as
//!   `mkIdent(free$x1)`. The value is the local itself, captured by the
//!   creator class;
//! * a type parameter with no tag in scope: a *free type*, `newFreeType`,
//!   placed as `mkTypeTree(TypeRef(NoPrefix, free$T1, Nil))`; with a tag,
//!   `mkTypeTree(tag.in[$u.type]($m).tpe)`;
//! * a nested `reify` is reified as the call it is -- the toolbox that later
//!   compiles the tree expands it itself.
//!
//! What this module does not build it names in the `Err`, and
//! `Check::report_reify_gap` reports it; nothing is approximated. The flag
//! words on free symbols are the exact values nsc writes (`FreeFlags`).

use std::collections::{HashMap, HashSet};

use scala_rs_parser::{Flags, Lit, Modifiers, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use super::{describe, Reifier};
use crate::symbol::{SymKind, SymbolTable};

/// What a `reify { … }` body needs beyond a quasiquote's: the symbol table
/// its references resolved against, and everything `Check` worked out about
/// the body before handing it over.
pub(crate) struct ReifyEnv<'a> {
    pub(crate) st: &'a SymbolTable,
    /// Symbols the body itself defines: `val`s, `def`s, classes, objects,
    /// type aliases, type parameters, parameters. These, and references to
    /// them, are reified by name.
    pub(crate) local_syms: HashSet<SymbolId>,
    /// Names of the types the body itself defines (classes, objects, type
    /// aliases, type parameters), for a written type that names one.
    pub(crate) local_type_names: HashSet<String>,
    /// The classes and objects enclosing the `reify`, innermost first.
    pub(crate) this_classes: Vec<SymbolId>,
    /// Type parameters / abstract types with a tag in scope, and the
    /// expression that tag is (`evidence$1`), typed at the expansion site.
    pub(crate) tags: HashMap<SymbolId, Tree>,
    /// Those of `tags` whose tag is a `TypeTag` rather than a `WeakTypeTag`:
    /// what decides whether the reification is concrete.
    pub(crate) strong_tags: HashSet<SymbolId>,
    /// The type each written type tree of the body resolved to, by node.
    pub(crate) types: HashMap<NodeId, Type>,
    /// A `reify` nested in the body, by the application node, with the
    /// (typed) expression naming its universe.
    pub(crate) nested: HashMap<NodeId, Tree>,
    /// The qualifier of each `<e>.splice`, by the selection node, as it was
    /// *written*: it is typed again as part of the expansion.
    pub(crate) splices: HashMap<NodeId, Tree>,
    /// Where each local symbol was defined, for a free symbol's origin.
    pub(crate) def_spans: HashMap<SymbolId, Span>,
    /// The file's name, for the same.
    pub(crate) file_name: String,
    /// `scala.reflect.api.Exprs.Expr`, to recognise `.splice`.
    pub(crate) expr_class: SymbolId,
}

/// The free symbols a body needs -- nsc's reifier symbol table.
///
/// `defs` are the `val free$x1 = newFreeTerm(...)` bindings, `infos` the
/// `setInfo(free$x1, ...)` statements that follow *all* of them, exactly the
/// order `SymbolTables.encode` writes them in.
#[derive(Default)]
pub(crate) struct Symtab {
    defs: Vec<Tree>,
    infos: Vec<Tree>,
    /// Free term / free type already bound, by symbol: the local's name.
    names: HashMap<SymbolId, String>,
    /// Free `this` per class.
    this_names: HashMap<SymbolId, String>,
    /// Per-prefix counters, the way nsc's fresh name creator numbers
    /// `free$x1`, `free$x2`, `free$y1`.
    counters: HashMap<String, u32>,
}

impl Symtab {
    fn fresh(&mut self, prefix: &str) -> String {
        let n = self.counters.entry(prefix.to_string()).or_insert(0);
        *n += 1;
        format!("{prefix}{n}")
    }

    /// Every binding, definitions first and then their infos.
    pub(crate) fn take(&mut self) -> Vec<Tree> {
        let mut out = std::mem::take(&mut self.defs);
        out.append(&mut self.infos);
        self.names.clear();
        self.this_names.clear();
        out
    }
}

/// The flag words nsc gives free symbols, read off `-Ymacro-debug-lite`
/// (`FlagsRepr(<n>L)`). Bit 44 (`TRIEDCOOKING`) is set on all of them.
mod free_flags {
    /// A method parameter: `PARAM | STABLE`.
    pub(super) const PARAM: i64 = 17592190246912;
    /// A local `val`: `STABLE`.
    pub(super) const VAL: i64 = 17592190238720;
    /// A local parameterless `def`: `METHOD | STABLE`.
    pub(super) const DEF: i64 = 17592190238784;
    /// A local `lazy val`: `LAZY | ACCESSOR | METHOD | STABLE`.
    pub(super) const LAZY_VAL: i64 = 17594471940160;
    /// A type parameter: `PARAM | DEFERRED`.
    pub(super) const TYPE_PARAM: i64 = 8208;
    /// An abstract type member: `DEFERRED`.
    pub(super) const ABSTRACT_TYPE: i64 = 16;
}

/// `Flags.PARAM | Flags.SYNTHETIC`, on the parameter nsc's parser invents for
/// a `_` placeholder (`x$1`); the same value `super::PLACEHOLDER_PARAM_FLAGS`.
const PLACEHOLDER: i64 = super::PLACEHOLDER_PARAM_FLAGS;

/// `Flags.FINAL | Flags.SYNTHETIC | Flags.ARTIFACT`, the modifiers on the
/// `val` a right-associative operator's left operand is bound to.
const RASSOC: i64 = super::RASSOC_VAL_FLAGS;

impl<'a> Reifier<'a> {
    fn env(&self) -> &ReifyEnv<'a> {
        &self.reify.as_ref().expect("reify mode").env
    }

    fn st(&self) -> &'a SymbolTable {
        self.reify.as_ref().expect("reify mode").env.st
    }

    fn mirror_local(&self) -> String {
        self.reify
            .as_ref()
            .expect("reify mode")
            .mirror_local
            .clone()
    }

    /// Every free-symbol binding the body asked for, in nsc's order.
    pub(crate) fn take_symtab(&self) -> Vec<Tree> {
        self.symtab.borrow_mut().take()
    }

    // -- terms ---------------------------------------------------------------

    /// The reify-mode reading of one term. `Ok(None)` only outside reify mode.
    pub(super) fn reify_term(&self, t: &Tree) -> Result<Option<Tree>, String> {
        if self.reify.is_none() {
            return Ok(None);
        }
        self.typed_term(t).map(Some)
    }

    fn typed_term(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Literal { lit } => Ok(match lit {
                // `'sym` is `scala.Symbol.apply("sym")` by the time the
                // reifier sees it.
                Lit::Symbol(s) => self.call(
                    self.universe_member("Apply"),
                    vec![
                        self.call(
                            self.universe_member("Select"),
                            vec![
                                self.static_module_ident("scala.Symbol"),
                                self.term_name("apply"),
                            ],
                        ),
                        self.list(vec![self.constant(Lit::String(s.clone()))]),
                    ],
                ),
                other => self.constant(other.clone()),
            }),
            TreeKind::Ident { .. } | TreeKind::Select { .. } | TreeKind::This { .. } => {
                self.term_ref(t)
            }
            TreeKind::Super { qual, mix } => Ok(self.super_ref(qual.as_deref(), mix.as_deref())),
            TreeKind::Apply { fun, args } => self.typed_apply(t, fun, args),
            TreeKind::TypeApply { fun, args } => {
                let f = self.term(fun)?;
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.typ(a)?);
                }
                Ok(self.call(self.universe_member("TypeApply"), vec![f, self.list(ts)]))
            }
            TreeKind::Function { vparams, body } => self.typed_function(vparams, body),
            TreeKind::Block { stats, expr } => {
                let mut elems: Vec<&Tree> = stats.iter().collect();
                if !expr.is_empty() {
                    elems.push(expr);
                }
                // Every definition the block binds is in scope for the whole
                // block; a `def` may be used before it is declared.
                let names: Vec<String> = elems
                    .iter()
                    .filter_map(|e| super::local_def_name(e))
                    .collect();
                self.with_locals(names, || {
                    let mut out = Vec::new();
                    for e in &elems {
                        out.push(self.stat(e)?);
                    }
                    let last = out.pop().unwrap_or_else(|| self.constant(Lit::Unit));
                    Ok(self.call(self.universe_member("Block"), vec![self.list(out), last]))
                })
            }
            TreeKind::If { cond, thenp, elsep } => {
                let c = self.term(cond)?;
                let a = self.term(thenp)?;
                let b = self.term(elsep)?;
                Ok(self.call(self.universe_member("If"), vec![c, a, b]))
            }
            TreeKind::Match { selector, cases } => {
                let sel = self.term(selector)?;
                let cs = self.case_defs(cases)?;
                Ok(self.call(self.universe_member("Match"), vec![sel, self.list(cs)]))
            }
            TreeKind::Typed { expr, tpt } => {
                let e = self.term(expr)?;
                let ty = self.typ(tpt)?;
                Ok(self.call(self.universe_member("Typed"), vec![e, ty]))
            }
            TreeKind::Assign { lhs, rhs } => {
                self.refuse_free_assignment(lhs)?;
                let l = self.term(lhs)?;
                let r = self.term(rhs)?;
                Ok(self.call(self.universe_member("Assign"), vec![l, r]))
            }
            TreeKind::While { cond, body } => {
                // nsc's typer: `LabelDef(while$1, Nil, If(cond, Block(List(body),
                // Apply(Ident(while$1), Nil)), Literal(())))`.
                let label = self.fresh_label("while$");
                let c = self.term(cond)?;
                let b = self.term(body)?;
                let again = self.call(
                    self.universe_member("Apply"),
                    vec![self.plain_ident(&label), self.list(vec![])],
                );
                let block = self.call(
                    self.universe_member("Block"),
                    vec![self.list(vec![b]), again],
                );
                let iff = self.call(
                    self.universe_member("If"),
                    vec![c, block, self.constant(Lit::Unit)],
                );
                Ok(self.call(
                    self.universe_member("LabelDef"),
                    vec![self.term_name(&label), self.list(vec![]), iff],
                ))
            }
            TreeKind::DoWhile { body, cond } => {
                // `LabelDef(doWhile$1, Nil, Block(List(body), If(cond,
                // Apply(Ident(doWhile$1), Nil), Literal(()))))`.
                let label = self.fresh_label("doWhile$");
                let b = self.term(body)?;
                let c = self.term(cond)?;
                let again = self.call(
                    self.universe_member("Apply"),
                    vec![self.plain_ident(&label), self.list(vec![])],
                );
                let iff = self.call(
                    self.universe_member("If"),
                    vec![c, again, self.constant(Lit::Unit)],
                );
                let block = self.call(self.universe_member("Block"), vec![self.list(vec![b]), iff]);
                Ok(self.call(
                    self.universe_member("LabelDef"),
                    vec![self.term_name(&label), self.list(vec![]), block],
                ))
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                let b = self.term(block)?;
                let cs = self.case_defs(catches)?;
                let f = if finalizer.is_empty() {
                    self.universe_member("EmptyTree")
                } else {
                    self.term(finalizer)?
                };
                Ok(self.call(self.universe_member("Try"), vec![b, self.list(cs), f]))
            }
            TreeKind::Throw { expr } => {
                let e = self.term(expr)?;
                Ok(self.call(self.universe_member("Throw"), vec![e]))
            }
            TreeKind::Return { expr } => {
                let e = if expr.is_empty() {
                    self.constant(Lit::Unit)
                } else {
                    self.term(expr)?
                };
                Ok(self.call(self.universe_member("Return"), vec![e]))
            }
            TreeKind::New { tpt } => self.typed_new(t, tpt, &[]),
            TreeKind::InterpolatedString {
                prefix,
                parts,
                args,
            } => {
                // `scala.StringContext.apply(parts...).<prefix>(args...)`:
                // nsc reifies the call, not the `s` macro's expansion
                // (`Reshape.undoMacroExpansion`).
                let ps: Vec<Tree> = parts
                    .iter()
                    .map(|p| self.constant(Lit::String(p.clone())))
                    .collect();
                let ctx = self.call(
                    self.universe_member("Apply"),
                    vec![
                        self.call(
                            self.universe_member("Select"),
                            vec![
                                self.static_module_ident("scala.StringContext"),
                                self.term_name("apply"),
                            ],
                        ),
                        self.list(ps),
                    ],
                );
                let mut as_ = Vec::new();
                for a in args {
                    as_.push(self.term(a)?);
                }
                Ok(self.call(
                    self.universe_member("Apply"),
                    vec![
                        self.call(
                            self.universe_member("Select"),
                            vec![ctx, self.term_name(prefix)],
                        ),
                        self.list(as_),
                    ],
                ))
            }
            TreeKind::ValDef { .. }
            | TreeKind::DefDef { .. }
            | TreeKind::ClassDef { .. }
            | TreeKind::ModuleDef { .. }
            | TreeKind::TypeDef { .. } => self.definition(t),
            TreeKind::Import { .. } => {
                Err("an `import` inside a reify body is not reified yet".into())
            }
            other => Err(format!("{} is not reified yet", describe(other))),
        }
    }

    /// `Apply`, with the typer's own wrappers seen through: `$box` / `$unbox`
    /// are erasure's, not the program's, and a nested `reify` is reified as
    /// the call it was.
    fn typed_apply(&self, t: &Tree, fun: &Tree, args: &[Tree]) -> Result<Tree, String> {
        if let TreeKind::Ident { name } = &fun.kind {
            if (name == "$box" || name == "$unbox") && args.len() == 1 && fun.sym.is_none() {
                return self.term(&args[0]);
            }
        }
        // A `ClassTag` the typer materialised (`ClassTag.apply($classOf)`):
        // nsc's reifier undoes the materialisation to `Predef.implicitly`
        // (`Reshape.undoMacroExpansion`), and so does this.
        if args
            .iter()
            .any(|a| matches!(&a.kind, TreeKind::Ident { name } if name == "$classOf"))
        {
            return Ok(self.call(
                self.universe_member("Select"),
                vec![
                    self.static_module_ident("scala.Predef"),
                    self.term_name("implicitly"),
                ],
            ));
        }
        // The typer's own applications: `x.unary_-` gets an empty argument
        // list, and an implicit clause is filled by appending its arguments
        // to the call. Neither was written, and whoever type-checks the
        // reified tree infers both again -- so a typer-made `Apply` with no
        // written arguments left is the callee alone.
        let args: Vec<&Tree> = self.written_args(fun, args);
        // No written argument, and a callee that is not a method waiting
        // for a clause (`-i` is `Apply(Select(i, unary_-), Nil)` after
        // parsing; `-1` folds to a literal): the application is not one.
        if args.is_empty()
            && (t.id == NodeId(0)
                || matches!(fun.kind, TreeKind::Literal { .. })
                || !matches!(&fun.ty, Type::Method { paramss, .. } if !paramss.is_empty()))
        {
            return self.term(fun);
        }
        // The typer keeps a node's id when it wraps it in an implicit
        // conversion, so the id alone does not say which `Apply` is the
        // nested `reify`: it is the one whose callee is spelled `reify`.
        let is_reify_call = matches!(&fun.kind,
            TreeKind::Ident { name } | TreeKind::Select { name, .. } if name == "reify");
        if let Some(universe) = self.env().nested.get(&t.id).filter(|_| is_reify_call) {
            let u = self.term(universe)?;
            let body = args.first().ok_or("a nested reify has no body")?;
            let b = self.term(body)?;
            return Ok(self.call(
                self.universe_member("Apply"),
                vec![
                    self.call(
                        self.universe_member("Select"),
                        vec![u, self.term_name("reify")],
                    ),
                    self.list(vec![b]),
                ],
            ));
        }
        // `new C(a)(b)`: the spine is folded into one `New` with the
        // constructor selected on it.
        let owned: Vec<Tree> = args.iter().map(|a| (*a).clone()).collect();
        let mut clauses: Vec<&[Tree]> = vec![&owned];
        let mut head = fun;
        while let TreeKind::Apply { fun, args } = &head.kind {
            clauses.push(args);
            head = fun;
        }
        if let TreeKind::New { tpt } = &head.kind {
            clauses.reverse();
            return self.typed_new(head, tpt, &clauses);
        }
        // `a :: b` written infix: nsc's parser binds the left operand first
        // unless it is safe to inline, and either way leaves a block.
        if let TreeKind::Select { qual, name } = &fun.kind {
            if self.written_infix(fun.span, name) {
                if let [lhs] = args.as_slice() {
                    return self.typed_right_assoc(qual, name, lhs);
                }
            }
        }
        let mut f = self.callee(fun)?;
        // Applying a *value* (`f(1)` for `val f: Int => Int`, `h(1)(2)` where
        // `h(1)` returns a function): nsc's typer selects `apply` on it, and
        // the reified tree carries that selection.
        if !fun.ty.is_no_type()
            && !fun.ty.is_error()
            && !matches!(fun.ty, Type::Method { .. } | Type::Overload(_))
        {
            f = self.call(
                self.universe_member("Select"),
                vec![f, self.term_name("apply")],
            );
        }
        let mut out = Vec::new();
        for a in args {
            out.push(self.term(a)?);
        }
        Ok(self.call(self.universe_member("Apply"), vec![f, self.list(out)]))
    }

    /// The callee of a written application: a reference, reified without
    /// the empty clause `with_empty_clause` would add.
    fn callee(&self, fun: &Tree) -> Result<Tree, String> {
        match &fun.kind {
            TreeKind::Select { qual, name } => self.select_ref(fun, qual, name),
            TreeKind::TypeApply { fun: inner, args } => {
                let f = self.callee(inner)?;
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.typ(a)?);
                }
                Ok(self.call(self.universe_member("TypeApply"), vec![f, self.list(ts)]))
            }
            _ => self.term(fun),
        }
    }

    /// The arguments of an application the program wrote.
    ///
    /// The typer fills a trailing implicit clause by appending its arguments
    /// to the call (`Array(1, 2)` becomes `Array.apply(1, 2, ClassTag.Int)`),
    /// and the callee's type still shows the clause: every argument beyond
    /// the first clause is dropped, so that whoever type-checks the reified
    /// tree infers the implicit again in its own scope (nsc keeps them, and
    /// re-infers them just the same). A method whose *only* clause is
    /// implicit (`xs.sum`) keeps its argument, as nsc's tree does.
    fn written_args<'t>(&self, fun: &Tree, args: &'t [Tree]) -> Vec<&'t Tree> {
        let mut keep: Vec<&Tree> = args.iter().collect();
        if let Type::Method { paramss, .. } = &fun.ty {
            if paramss.len() >= 2 {
                let implicit: usize = paramss[1..].iter().map(|c| c.len()).sum();
                if keep.len() >= implicit {
                    keep.truncate(keep.len() - implicit);
                }
            }
        }
        keep
    }

    /// `{ val rassoc$1 = a; b.::(rassoc$1) }`, or `{ b.::(a) }` (an empty
    /// block) when `a` is a literal, a name or `this` -- nsc's parser
    /// (`makeBinop`, `isExprSafeToInline`).
    fn typed_right_assoc(&self, qual: &Tree, name: &str, lhs: &Tree) -> Result<Tree, String> {
        let safe = matches!(
            lhs.kind,
            TreeKind::Literal { .. } | TreeKind::Ident { .. } | TreeKind::This { .. }
        );
        let recv = self.term(qual)?;
        if safe {
            let arg = self.term(lhs)?;
            let app = self.call(
                self.universe_member("Apply"),
                vec![
                    self.call(
                        self.universe_member("Select"),
                        vec![recv, self.term_name(name)],
                    ),
                    self.list(vec![arg]),
                ],
            );
            return Ok(self.call(self.universe_member("Block"), vec![self.list(vec![]), app]));
        }
        let local = self.fresh_label("rassoc$");
        let bound = self.call(
            self.universe_member("ValDef"),
            vec![
                self.mods(RASSOC),
                self.term_name(&local),
                self.empty_type_tree(),
                self.term(lhs)?,
            ],
        );
        let app = self.call(
            self.universe_member("Apply"),
            vec![
                self.call(
                    self.universe_member("Select"),
                    vec![recv, self.term_name(name)],
                ),
                self.list(vec![self.plain_ident(&local)]),
            ],
        );
        Ok(self.call(
            self.universe_member("Block"),
            vec![self.list(vec![bound]), app],
        ))
    }

    /// `Apply(Select(New(<type>), termNames.CONSTRUCTOR), args)`, one `Apply`
    /// per clause; `new C { ... }` is a block binding the anonymous class.
    fn typed_new(&self, new: &Tree, tpt: &Tree, clauses: &[&[Tree]]) -> Result<Tree, String> {
        // The parser folds the first argument clause into the `New` itself.
        let mut inner: Vec<&[Tree]> = Vec::new();
        let mut head = tpt;
        while let TreeKind::Apply { fun, args } = &head.kind {
            inner.push(args);
            head = fun;
        }
        inner.reverse();
        inner.extend_from_slice(clauses);
        if let TreeKind::ClassDef { .. } = &head.kind {
            return self.typed_anon_new(head, &inner);
        }
        let ty = match self.env().types.get(&head.id) {
            _ if self.mentions_local_type(head) => Type::NoType,
            Some(ty) => ty.clone(),
            None if !new.ty.is_no_type() => new.ty.clone(),
            None => Type::NoType,
        };
        let tpt_tree = if ty.is_no_type() || ty.is_error() {
            self.typ(head)?
        } else {
            self.type_tree_of_type(&self.st().dealias(&ty))?
        };
        let mut cur = self.call(
            self.universe_member("Select"),
            vec![
                self.call(self.universe_member("New"), vec![tpt_tree]),
                self.select(self.universe_member("termNames"), "CONSTRUCTOR"),
            ],
        );
        if inner.is_empty() {
            inner.push(&[]);
        }
        for clause in inner {
            let mut out = Vec::new();
            for a in clause {
                out.push(self.term(a)?);
            }
            cur = self.call(self.universe_member("Apply"), vec![cur, self.list(out)]);
        }
        Ok(cur)
    }

    /// `new C(1) { ... }`: nsc's typer names the class `$anon` and binds it in
    /// a block, `{ final class $anon extends C(1) { ... }; new $anon() }`.
    fn typed_anon_new(&self, class: &Tree, clauses: &[&[Tree]]) -> Result<Tree, String> {
        // The parser keeps a parent's constructor arguments in the template.
        if clauses.iter().any(|c| !c.is_empty()) {
            return Err("an anonymous class with constructor arguments is not reified yet".into());
        }
        let def = self.definition(class)?;
        let make = self.call(
            self.universe_member("Apply"),
            vec![
                self.call(
                    self.universe_member("Select"),
                    vec![
                        self.call(
                            self.universe_member("New"),
                            vec![self.call(
                                self.universe_member("Ident"),
                                vec![self.type_name("$anon")],
                            )],
                        ),
                        self.select(self.universe_member("termNames"), "CONSTRUCTOR"),
                    ],
                ),
                self.list(vec![]),
            ],
        );
        Ok(self.call(
            self.universe_member("Block"),
            vec![self.list(vec![def]), make],
        ))
    }

    /// `Function(List(ValDef(Modifiers(PARAM), name, tpt, EmptyTree)), body)`.
    /// The parameters are local to the body and reified by name; a `_`
    /// placeholder's invented `x$1` keeps its name and its `SYNTHETIC` flag.
    fn typed_function(&self, vparams: &[Tree], body: &Tree) -> Result<Tree, String> {
        let mut ps = Vec::new();
        let mut names = Vec::new();
        for p in vparams {
            let TreeKind::ValDef {
                mods, name, tpt, ..
            } = &p.kind
            else {
                return Err("a function literal's parameter is not reified yet".into());
            };
            let flags = if super::is_parser_placeholder(mods.flags, name) {
                PLACEHOLDER
            } else if mods.flags == Flags::PARAM
                || mods.flags == Flags::PARAM.with(Flags::SYNTHETIC)
            {
                super::PARAM_FLAGS
            } else {
                return Err("a modified function literal parameter is not reified yet".into());
            };
            names.push(name.clone());
            let ty = match self.env().types.get(&tpt.id) {
                Some(ty) => Some(ty.clone()),
                None if tpt.is_empty() => None,
                None if !p.ty.is_no_type() && !p.ty.is_error() => Some(p.ty.clone()),
                None => None,
            };
            let tpt_tree = match ty {
                Some(ty) => self.type_tree_of_type(&ty)?,
                None => self.typ(tpt)?,
            };
            ps.push(self.call(
                self.universe_member("ValDef"),
                vec![
                    self.mods(flags),
                    self.term_name(name),
                    tpt_tree,
                    self.universe_member("EmptyTree"),
                ],
            ));
        }
        let b = self.with_locals(names, || self.term(body))?;
        Ok(self.call(self.universe_member("Function"), vec![self.list(ps), b]))
    }

    /// An assignment to a `var` bound outside the body cannot be carried:
    /// nsc boxes the variable and reifies `free.elem`, and scala-rs carries a
    /// free term by value. Reads are fine; a write would be lost.
    fn refuse_free_assignment(&self, lhs: &Tree) -> Result<(), String> {
        if let TreeKind::Ident { name } = &lhs.kind {
            if !lhs.sym.is_none() && !self.is_local(lhs.sym) && !self.local_bound(name) {
                let s = self.st().get(lhs.sym);
                if matches!(s.kind, SymKind::Term) && !self.st().get(s.owner).is_class_like() {
                    return Err(format!(
                        "an assignment to `{name}`, a `var` bound outside the reify body, is not reified yet"
                    ));
                }
            }
        }
        Ok(())
    }

    // -- references ----------------------------------------------------------

    fn is_local(&self, sym: SymbolId) -> bool {
        !sym.is_none() && self.env().local_syms.contains(&sym)
    }

    /// One `Ident` / `Select` / `This`, rebuilt from the symbol it resolved to.
    fn term_ref(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::This { qual } => {
                let cls = self
                    .st()
                    .class_sym_of(&t.ty)
                    .or_else(|| (!t.sym.is_none()).then_some(t.sym))
                    .ok_or_else(|| "`this` could not be resolved".to_string())?;
                self.this_ref(cls, qual.as_deref())
            }
            TreeKind::Ident { name } => {
                if self.is_local(t.sym) {
                    return Ok(self.plain_ident(name));
                }
                if let Some(local) = self.placeholder_param(name) {
                    return Ok(self.call(
                        self.support_member("SyntacticTermIdent"),
                        vec![self.local(&local), self.lit(Lit::Boolean(false))],
                    ));
                }
                if self.local_bound(name) {
                    return Ok(self.plain_ident(name));
                }
                if t.sym.is_none() {
                    return Err(format!("`{name}` could not be resolved"));
                }
                self.symbol_ref(t.sym, name)
            }
            TreeKind::Select { qual, name } => {
                let built = self.select_ref(t, qual, name)?;
                Ok(self.with_empty_clause(t, built))
            }
            _ => unreachable!("term_ref is only called for references"),
        }
    }

    /// `f.m()` for a selection of a `def m()` the typer auto-applied: nsc's
    /// typed tree carries the empty argument list (`s.length()`), and so does
    /// the reified one. A callee (`typed_apply` reifies those itself) and a
    /// method the typer left unapplied are left alone.
    fn with_empty_clause(&self, t: &Tree, built: Tree) -> Tree {
        if t.sym.is_none() || matches!(t.ty, Type::Method { .. }) {
            return built;
        }
        let s = self.st().get(t.sym);
        let empty_clause = s.parameterless_method == Some(false)
            && matches!(&s.ty, Type::Method { paramss, .. }
                if paramss.len() == 1 && paramss[0].is_empty());
        if !empty_clause {
            return built;
        }
        self.call(
            self.universe_member("Apply"),
            vec![built, self.list(vec![])],
        )
    }

    fn select_ref(&self, t: &Tree, qual: &Tree, name: &str) -> Result<Tree, String> {
        {
            {
                // `x.splice`: the argument's own tree, rebased into the
                // mirror the creator was handed.
                if name == "splice" {
                    let is_expr = matches!(&qual.ty, Type::Class { sym, .. }
                        if *sym == self.env().expr_class);
                    if is_expr {
                        if let Some(written) = self.env().splices.get(&t.id) {
                            let ctx = self.reify.as_ref().expect("reify mode");
                            return Ok(self.splice_tree(ctx, written));
                        }
                    }
                }
                if self.is_local(t.sym) {
                    let q = self.term(qual)?;
                    return Ok(self.call(
                        self.universe_member("Select"),
                        vec![q, self.term_name(name)],
                    ));
                }
                // `scala.collection.immutable.List`: a package prefix is not a
                // tree, the selected symbol is.
                if !qual.sym.is_none() && self.st().get(qual.sym).kind == SymKind::Package {
                    if t.sym.is_none() {
                        return Err(format!("`{name}` could not be resolved"));
                    }
                    return self.symbol_ref(t.sym, name);
                }
                if let Type::ModuleRef(p) = &qual.ty {
                    if self.st().get(*p).kind == SymKind::Package && !t.sym.is_none() {
                        return self.symbol_ref(t.sym, name);
                    }
                }
                let q = self.term(qual)?;
                let member = if t.sym.is_none() {
                    name.to_string()
                } else {
                    self.st().get(t.sym).name.clone()
                };
                Ok(self.call(
                    self.universe_member("Select"),
                    vec![q, self.term_name(&member)],
                ))
            }
        }
    }

    /// A bare name that resolved to `sym`: a static `object`, a member of one,
    /// a member of an enclosing class, or a free term.
    fn symbol_ref(&self, sym: SymbolId, written: &str) -> Result<Tree, String> {
        let st = self.st();
        let s = st.get(sym);
        match s.kind {
            SymKind::Module | SymKind::ModuleClass => {
                let mcls = st.module_class_of(sym);
                if let Some((home, alias)) = self.scala_alias(mcls) {
                    return Ok(self.call(
                        self.universe_member("Select"),
                        vec![self.static_module_ident(home), self.term_name(&alias)],
                    ));
                }
                if let Some(full) = self.static_module_full_name(mcls) {
                    return Ok(self.static_module_ident(&full));
                }
                // `object O` nested in an enclosing class, or in another
                // static object.
                self.member_ref(sym, &s.name)
            }
            SymKind::Package => {
                let full = self.dotted_name(sym);
                Ok(self.call(
                    self.support_member("mkIdent"),
                    vec![self.call(
                        self.select(self.local(&self.mirror_local()), "staticPackage"),
                        vec![self.lit(Lit::String(full))],
                    )],
                ))
            }
            SymKind::Method | SymKind::Term => self.member_ref(sym, written),
            SymKind::Class => Err(format!(
                "`{written}`, a class used as a value, is not reified yet"
            )),
            SymKind::TypeParam | SymKind::TypeMember | SymKind::NoSymbol => Err(format!(
                "`{written}`, a type used as a value, is not reified yet"
            )),
        }
    }

    /// A term member or local reached without a qualifier.
    fn member_ref(&self, sym: SymbolId, written: &str) -> Result<Tree, String> {
        let st = self.st();
        let s = st.get(sym);
        let owner = st.get(s.owner);
        // A member of a class or object that encloses the `reify`: nsc's
        // typer spells it `C.this.x`.
        if let Some(cls) = self.enclosing_owner_of(sym) {
            let this = self.this_ref(cls, None)?;
            return Ok(self.call(
                self.universe_member("Select"),
                vec![this, self.term_name(&s.name)],
            ));
        }
        match owner.kind {
            SymKind::ModuleClass | SymKind::Module if self.is_member(sym) => {
                let mcls = st.module_class_of(s.owner);
                let Some(full) = self.static_module_full_name(mcls) else {
                    return Err(format!(
                        "`{written}`, a member of an `object` that is neither static nor enclosing, is not reified yet"
                    ));
                };
                // The prelude spells `Predef`'s `->` conversion by its 2.10
                // name; the library's method is `ArrowAssoc`.
                let member = if full == "scala.Predef" && s.name == "any2ArrowAssoc" {
                    "ArrowAssoc".to_string()
                } else {
                    s.name.clone()
                };
                // `List.apply`: the object is reached through its alias in
                // `scala`'s package object, as a bare `List` is.
                let owner_ref = match self.scala_alias(mcls) {
                    Some((home, alias)) => self.call(
                        self.universe_member("Select"),
                        vec![self.static_module_ident(home), self.term_name(&alias)],
                    ),
                    None => self.static_module_ident(&full),
                };
                Ok(self.call(
                    self.universe_member("Select"),
                    vec![owner_ref, self.term_name(&member)],
                ))
            }
            // A package object's member lives on the package here;
            // nsc reaches it through `staticModule("p.package")`.
            SymKind::Package => {
                let full = format!("{}.package", self.dotted_name(s.owner));
                Ok(self.call(
                    self.universe_member("Select"),
                    vec![self.static_module_ident(&full), self.term_name(&s.name)],
                ))
            }
            SymKind::Class if self.is_member(sym) => Err(format!(
                "`{written}`, a member of a class that does not enclose the reify, is not reified yet"
            )),
            // Bound by a method, or by a block in a template, outside the
            // body: a free term.
            _ => self.free_term(sym),
        }
    }

    /// The enclosing class or object `sym` is a member of, if any.
    fn enclosing_owner_of(&self, sym: SymbolId) -> Option<SymbolId> {
        let st = self.st();
        if !self.is_member(sym) {
            return None;
        }
        self.env()
            .this_classes
            .iter()
            .copied()
            .find(|&cls| st.lookup_member(cls, &st.get(sym).name).contains(&sym))
    }

    /// Whether `sym` is a *member* of the class that owns it, as opposed to
    /// a local of a block in that class's template: the typer gives both the
    /// class as owner, and only a member is in its member list.
    fn is_member(&self, sym: SymbolId) -> bool {
        let st = self.st();
        let owner = st.get(sym).owner;
        st.get(owner).is_class_like() && st.lookup_member(owner, &st.get(sym).name).contains(&sym)
    }

    /// `this` of `cls`: by name for a class the body defines, `mkThis` for a
    /// static object, a free term for an enclosing class.
    fn this_ref(&self, cls: SymbolId, written: Option<&str>) -> Result<Tree, String> {
        let st = self.st();
        if self.is_local(cls) {
            return Ok(self.call(
                self.universe_member("This"),
                vec![self.type_name(written.unwrap_or(&st.get(cls).name))],
            ));
        }
        let s = st.get(cls);
        if matches!(s.kind, SymKind::ModuleClass | SymKind::Module) {
            let mcls = st.module_class_of(cls);
            if let Some(full) = self.static_module_full_name(mcls) {
                return Ok(self.call(
                    self.support_member("mkThis"),
                    vec![self.module_class_of_static(&full)],
                ));
            }
            return Err(format!(
                "`this` of `{}`, an `object` that is not static, is not reified yet",
                s.name
            ));
        }
        if s.kind == SymKind::Class && self.is_static_class(cls) {
            return self.free_this(cls);
        }
        Err(format!(
            "`this` of `{}`, a class that is not static, is not reified yet",
            s.name
        ))
    }

    /// `$m.staticModule("<full>").asModule.moduleClass`.
    fn module_class_of_static(&self, full: &str) -> Tree {
        self.select(
            self.select(
                self.call(
                    self.select(self.local(&self.mirror_local()), "staticModule"),
                    vec![self.lit(Lit::String(full.to_string()))],
                ),
                "asModule",
            ),
            "moduleClass",
        )
    }

    /// `rs.mkIdent($m.staticModule("<full>"))`.
    fn static_module_ident(&self, full: &str) -> Tree {
        self.call(
            self.support_member("mkIdent"),
            vec![self.call(
                self.select(self.local(&self.mirror_local()), "staticModule"),
                vec![self.lit(Lit::String(full.to_string()))],
            )],
        )
    }

    /// `$m.staticClass("<full>")`.
    fn static_class(&self, full: &str) -> Tree {
        self.call(
            self.select(self.local(&self.mirror_local()), "staticClass"),
            vec![self.lit(Lit::String(full.to_string()))],
        )
    }

    /// `$m.staticPackage("<full>").asModule.moduleClass`.
    fn package_class(&self, full: &str) -> Tree {
        self.select(
            self.select(
                self.call(
                    self.select(self.local(&self.mirror_local()), "staticPackage"),
                    vec![self.lit(Lit::String(full.to_string()))],
                ),
                "asModule",
            ),
            "moduleClass",
        )
    }

    /// The `Mirror.staticModule` name of a static module class, if it is one.
    fn static_module_full_name(&self, mcls: SymbolId) -> Option<String> {
        let st = self.st();
        if st.get(mcls).kind != SymKind::ModuleClass {
            return None;
        }
        if !self.is_static_owner_chain(st.get(mcls).owner) {
            return None;
        }
        let jvm = st.jvm_internal(mcls);
        let full = jvm.strip_suffix('$').unwrap_or(&jvm);
        if full.is_empty() || full.rsplit('/').next().is_some_and(|s| s.contains('$')) {
            return None;
        }
        Some(full.replace('/', "."))
    }

    /// Whether every owner up from `owner` is a package or a static object.
    fn is_static_owner_chain(&self, mut owner: SymbolId) -> bool {
        let st = self.st();
        let mut guard = 0;
        while !owner.is_none() && guard < 32 {
            let o = st.get(owner);
            match o.kind {
                SymKind::Package => {}
                SymKind::ModuleClass | SymKind::Module => {
                    if self.is_local(owner) {
                        return false;
                    }
                }
                _ => return false,
            }
            if o.owner == owner {
                break;
            }
            owner = o.owner;
            guard += 1;
        }
        true
    }

    /// A class reachable by `staticClass` or by `selectType` on a static
    /// object: its owner chain is packages and static objects only.
    fn is_static_class(&self, cls: SymbolId) -> bool {
        let s = self.st().get(cls);
        s.kind == SymKind::Class && !self.is_local(cls) && self.is_locatable_owner_chain(s.owner)
    }

    /// nsc's `isLocatable`: every owner is a package, a static object, or a
    /// class that is itself locatable -- never a method or a local.
    fn is_locatable_owner_chain(&self, mut owner: SymbolId) -> bool {
        let st = self.st();
        let mut guard = 0;
        while !owner.is_none() && guard < 32 {
            let o = st.get(owner);
            match o.kind {
                SymKind::Package => {}
                SymKind::ModuleClass | SymKind::Module | SymKind::Class => {
                    if self.is_local(owner) {
                        return false;
                    }
                }
                _ => return false,
            }
            if o.owner == owner {
                break;
            }
            owner = o.owner;
            guard += 1;
        }
        true
    }

    /// The dotted full name of a package or a symbol under packages.
    fn dotted_name(&self, sym: SymbolId) -> String {
        let st = self.st();
        let mut parts = Vec::new();
        let mut cur = sym;
        let mut guard = 0;
        while !cur.is_none() && guard < 32 {
            let s = st.get(cur);
            if s.name.is_empty()
                || s.name == "<_root_>"
                || s.name == "_root_"
                || s.name == "<empty>"
            {
                break;
            }
            parts.push(s.name.trim_end_matches('$').to_string());
            if s.owner == cur {
                break;
            }
            cur = s.owner;
            guard += 1;
        }
        parts.reverse();
        parts.join(".")
    }

    // -- free symbols --------------------------------------------------------

    /// `rs.mkIdent(free$x1)`, binding the free term on first use.
    fn free_term(&self, sym: SymbolId) -> Result<Tree, String> {
        if let Some(n) = self.symtab.borrow().names.get(&sym) {
            return Ok(self.call(self.support_member("mkIdent"), vec![self.local(n)]));
        }
        let st = self.st();
        let s = st.get(sym);
        let name = s.name.clone();
        let (flags, info_ty) = match s.kind {
            SymKind::Term if s.flags.contains(Flags::PARAM) => (free_flags::PARAM, s.ty.clone()),
            SymKind::Term if s.flags.contains(Flags::LAZY) => (
                free_flags::LAZY_VAL,
                Type::Method {
                    paramss: vec![],
                    ret: Box::new(s.ty.clone()),
                },
            ),
            SymKind::Term => (free_flags::VAL, s.ty.clone()),
            SymKind::Method => match &s.ty {
                Type::Method { paramss, ret } if paramss.is_empty() => (
                    free_flags::DEF,
                    Type::Method {
                        paramss: vec![],
                        ret: ret.clone(),
                    },
                ),
                Type::Method { .. } => {
                    return Err(format!(
                        "`{name}`, a local method with parameters, is not reified yet"
                    ))
                }
                other => (
                    free_flags::DEF,
                    Type::Method {
                        paramss: vec![],
                        ret: Box::new(other.clone()),
                    },
                ),
            },
            _ => {
                return Err(format!(
                    "`{name}`, a {} bound outside the reify body, is not reified yet",
                    kind_word(s.kind)
                ))
            }
        };
        let info = self.type_value(&info_ty)?;
        let local = self.symtab.borrow_mut().fresh(&format!("free${name}"));
        let origin = self.symbol_origin(sym);
        let binding = self.plain_source_ident(&name);
        self.bind_free(
            &local,
            "newFreeTerm",
            Some(binding),
            flags,
            &origin,
            info,
            &name,
        );
        self.symtab.borrow_mut().names.insert(sym, local.clone());
        Ok(self.call(self.support_member("mkIdent"), vec![self.local(&local)]))
    }

    /// `rs.mkIdent(free$C$this)`: `this` of an enclosing class, carried as a
    /// free term whose value is `C.this`.
    fn free_this(&self, cls: SymbolId) -> Result<Tree, String> {
        if let Some(n) = self.symtab.borrow().this_names.get(&cls) {
            return Ok(self.call(self.support_member("mkIdent"), vec![self.local(n)]));
        }
        let st = self.st();
        let name = st.get(cls).name.clone();
        if !st.get(cls).tparams.is_empty() {
            return Err(format!(
                "`this` of `{name}`, a class with type parameters, is not reified yet"
            ));
        }
        let info = self.class_type_value(cls, &[])?;
        let local = self.symtab.borrow_mut().fresh(&format!("free${name}$this"));
        let origin = self.symbol_origin(cls);
        let binding = self.node(TreeKind::This {
            qual: Some(name.clone()),
        });
        self.bind_free(
            &local,
            "newFreeTerm",
            Some(binding),
            0,
            &origin,
            info,
            &name,
        );
        self.symtab
            .borrow_mut()
            .this_names
            .insert(cls, local.clone());
        Ok(self.call(self.support_member("mkIdent"), vec![self.local(&local)]))
    }

    /// The local bound to the free type for `sym`, binding it on first use.
    fn free_type(&self, sym: SymbolId) -> Result<String, String> {
        if let Some(n) = self.symtab.borrow().names.get(&sym) {
            return Ok(n.clone());
        }
        let st = self.st();
        let s = st.get(sym);
        let name = s.name.clone();
        let flags = match s.kind {
            SymKind::TypeParam => free_flags::TYPE_PARAM,
            SymKind::TypeMember => free_flags::ABSTRACT_TYPE,
            _ => return Err(format!("`{name}` is not an abstract type")),
        };
        let local = self.symtab.borrow_mut().fresh(&format!("free${name}"));
        // Registered before the bounds are built: a bound may mention the
        // type itself.
        self.symtab.borrow_mut().names.insert(sym, local.clone());
        let (lo, hi) = self.type_param_bounds(sym);
        let lo = self.type_value(&lo)?;
        let hi = self.type_value(&hi)?;
        let info = self.call(self.support_member("TypeBounds"), vec![lo, hi]);
        let origin = self.symbol_origin(sym);
        self.bind_free(&local, "newFreeType", None, flags, &origin, info, &name);
        Ok(local)
    }

    /// Emit `val <local> = rs.<factory>("<name>", [binding,] rs.FlagsRepr(flags),
    /// "<origin>")` and `rs.setInfo(<local>, <info>)`.
    #[allow(clippy::too_many_arguments)]
    fn bind_free(
        &self,
        local: &str,
        factory: &str,
        binding: Option<Tree>,
        flags: i64,
        origin: &str,
        info: Tree,
        name: &str,
    ) {
        let mut args = vec![self.lit(Lit::String(name.to_string()))];
        if let Some(b) = binding {
            args.push(b);
        }
        args.push(self.call(
            self.support_member("FlagsRepr"),
            vec![self.lit(Lit::Long(flags))],
        ));
        args.push(self.lit(Lit::String(origin.to_string())));
        let def = self.node(TreeKind::ValDef {
            mods: Modifiers::default(),
            name: local.to_string(),
            tpt: Box::new(self.node(TreeKind::Empty)),
            rhs: Box::new(self.call(self.support_member(factory), args)),
        });
        let set = self.call(
            self.support_member("setInfo"),
            vec![self.local(local), info],
        );
        let mut symtab = self.symtab.borrow_mut();
        symtab.defs.push(def);
        symtab.infos.push(set);
    }

    /// The bounds of a type parameter, `Nothing` / `Any` when it has none.
    fn type_param_bounds(&self, sym: SymbolId) -> (Type, Type) {
        let s = self.st().get(sym);
        let (mut lo, mut hi) = (Type::Nothing, Type::Any);
        if let Some(b) = &s.bound_lo {
            lo = b.clone();
        }
        if let Some(b) = &s.bound_hi {
            hi = b.clone();
        }
        (lo, hi)
    }

    /// nsc's `origin(sym)`: `defined by <owner> in <file>:<line>:<col>`.
    fn symbol_origin(&self, sym: SymbolId) -> String {
        let st = self.st();
        let s = st.get(sym);
        let mut out = String::new();
        if !s.owner.is_none() {
            let o = st.get(s.owner);
            let owner_name = match o.kind {
                // A definition in a template-level block belongs to the
                // class's local dummy, which nsc prints as `<local C>`.
                SymKind::Class | SymKind::ModuleClass | SymKind::Module
                    if !matches!(s.kind, SymKind::TypeParam | SymKind::TypeMember) =>
                {
                    format!("<local {}>", o.name.trim_end_matches('$'))
                }
                _ => o.name.trim_end_matches('$').to_string(),
            };
            out.push_str(&format!("defined by {owner_name}"));
        }
        if let Some(span) = self.env().def_spans.get(&sym) {
            if let Some((line, col)) = self.name_position(*span, &s.name) {
                out.push_str(&format!(" in {}:{}:{}", self.env().file_name, line, col));
            }
        }
        if out.is_empty() {
            out.push_str("of unknown origin");
        }
        out
    }

    /// Line and column (1-based) of `name` inside the definition at `span`.
    fn name_position(&self, span: Span, name: &str) -> Option<(usize, usize)> {
        let (lo, hi) = (span.lo.to_usize(), span.hi.to_usize());
        let text = self.src.get(lo..hi)?;
        let bytes = text.as_bytes();
        let mut from = 0;
        let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
        let at = loop {
            let i = text[from..].find(name)? + from;
            let before = i == 0 || !is_ident(bytes[i - 1]);
            let after = i + name.len() >= bytes.len() || !is_ident(bytes[i + name.len()]);
            if before && after {
                break lo + i;
            }
            from = i + 1;
            if from >= text.len() {
                return None;
            }
        };
        let head = self.src.get(..at)?;
        let line = head.matches('\n').count() + 1;
        let col = head
            .rsplit('\n')
            .next()
            .map(|l| l.chars().count())
            .unwrap_or(0)
            + 1;
        Some((line, col))
    }

    // -- types ---------------------------------------------------------------

    /// One written type of the body (`Reifier::typ` in reify mode).
    pub(super) fn reify_typ(&self, t: &Tree) -> Result<Tree, String> {
        if t.is_empty() {
            return Ok(self.empty_type_tree());
        }
        // A written type that names something the body defines is rebuilt
        // by name, keeping its written path (`outer.D`): that is what nsc
        // does for a type local to the reifee, and the only scope in which
        // the name resolves is the body's own.
        if self.mentions_local_type(t) {
            return self.type_tree_by_name(t);
        }
        if let Some(ty) = self.env().types.get(&t.id) {
            let ty = self.st().dealias(ty);
            return self.type_tree_of_type(&ty);
        }
        if !t.ty.is_no_type() && !t.ty.is_error() && !matches!(t.ty, Type::Method { .. }) {
            let ty = self.st().dealias(&t.ty);
            return self.type_tree_of_type(&ty);
        }
        self.type_tree_by_name(t)
    }

    /// Whether a written type names a type the body defines, or is a path
    /// through a value the body binds.
    fn mentions_local_type(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Ident { name } => self.env().local_type_names.contains(name),
            TreeKind::AppliedTypeTree { tpt, args } => {
                self.mentions_local_type(tpt) || args.iter().any(|a| self.mentions_local_type(a))
            }
            TreeKind::SelectFromTypeTree { qual, hash, .. } if !*hash => self.local_path(qual),
            TreeKind::SelectFromTypeTree { qual, .. } => self.mentions_local_type(qual),
            TreeKind::Select { qual, .. } => self.local_path(qual),
            TreeKind::SingletonTypeTree { ref_ } => self.local_path(ref_),
            TreeKind::CompoundTypeTree { parents, .. } => {
                parents.iter().any(|p| self.mentions_local_type(p))
            }
            _ => false,
        }
    }

    /// Whether a term path starts at a value the body binds.
    fn local_path(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Ident { name } => self.is_local(t.sym) || self.local_bound(name),
            TreeKind::Select { qual, .. } => self.local_path(qual),
            TreeKind::This { .. } => {
                self.is_local(t.sym)
                    || self
                        .st()
                        .class_sym_of(&t.ty)
                        .is_some_and(|c| self.is_local(c))
            }
            _ => false,
        }
    }

    /// A written type built by name: only allowed to name what the body
    /// itself defines, where by-name is nsc's own answer.
    fn type_tree_by_name(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::Ident { name } if self.env().local_type_names.contains(name) => {
                Ok(self.call(self.universe_member("Ident"), vec![self.type_name(name)]))
            }
            TreeKind::Ident { name } => {
                if let Some(local) = self.wildcard_local(t.id) {
                    return Ok(self.call(
                        self.support_member("SyntacticTypeIdent"),
                        vec![self.local(&local)],
                    ));
                }
                Err(format!("the type `{name}` could not be resolved"))
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                let head = self.reify_typ(tpt)?;
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.reify_typ(a)?);
                }
                Ok(self.call(
                    self.universe_member("AppliedTypeTree"),
                    vec![head, self.list(ts)],
                ))
            }
            // `p.T` where `p` is a value the body binds.
            TreeKind::SelectFromTypeTree { qual, name, hash } if !*hash => {
                let q = self.term(qual)?;
                Ok(self.call(
                    self.universe_member("Select"),
                    vec![q, self.type_name(name)],
                ))
            }
            TreeKind::Select { qual, name } => {
                let q = self.term(qual)?;
                Ok(self.call(
                    self.universe_member("Select"),
                    vec![q, self.type_name(name)],
                ))
            }
            TreeKind::SingletonTypeTree { ref_ } => {
                let r = self.term(ref_)?;
                Ok(self.call(self.universe_member("SingletonTypeTree"), vec![r]))
            }
            other => Err(format!(
                "{} in a `reify` body could not be resolved",
                super::describe_type(other)
            )),
        }
    }

    /// The tree for a resolved type in a type position: a leaf naming a
    /// class is `mkIdent(...)`, an application `AppliedTypeTree`, and an
    /// abstract type is spliced (`mkTypeTree(...)`) -- nsc's `reifyBoundType`.
    fn type_tree_of_type(&self, ty: &Type) -> Result<Tree, String> {
        let st = self.st();
        // An inner class carrying its prefix as an as-seen-from view
        // (`crate::prefix`): the tree names the class; the prefix matters
        // for the type value only.
        if crate::prefix::view_prefix(ty).is_some() {
            return self.type_tree_of_type(crate::prefix::strip_view(ty));
        }
        match ty {
            Type::Constant(l) => self.type_tree_of_type(&Type::lit_underlying(l)),
            Type::String => Ok(self.call(
                self.universe_member("Select"),
                vec![
                    self.static_module_ident("scala.Predef"),
                    self.type_name("String"),
                ],
            )),
            Type::AnyRef | Type::JavaObject => Ok(self.call(
                self.support_member("mkIdent"),
                vec![self.call(
                    self.support_member("selectType"),
                    vec![
                        self.package_class("scala"),
                        self.lit(Lit::String("AnyRef".into())),
                    ],
                )],
            )),
            Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Char
            | Type::Int
            | Type::Long
            | Type::Float
            | Type::Double
            | Type::Unit
            | Type::Any
            | Type::AnyVal
            | Type::Nothing
            | Type::Null => {
                let name = crate::materialize::static_class_name(st, ty)?;
                Ok(self.call(
                    self.support_member("mkIdent"),
                    vec![self.static_class(&name)],
                ))
            }
            Type::Class { sym, args } => {
                let head = self.class_type_tree(*sym)?;
                self.applied_tree(head, args)
            }
            Type::Function { params, ret } => {
                let head = self.call(
                    self.support_member("mkIdent"),
                    vec![self.static_class(&format!("scala.Function{}", params.len()))],
                );
                let mut all: Vec<Type> = params.clone();
                all.push((**ret).clone());
                self.applied_tree(head, &all)
            }
            Type::Tuple(xs) => {
                let head = self.call(
                    self.support_member("mkIdent"),
                    vec![self.static_class(&format!("scala.Tuple{}", xs.len()))],
                );
                self.applied_tree(head, xs)
            }
            Type::Array(elem) => {
                let head = self.call(
                    self.support_member("mkIdent"),
                    vec![self.static_class("scala.Array")],
                );
                self.applied_tree(head, std::slice::from_ref(elem))
            }
            Type::TypeParam(id) | Type::TypeMember(id) => {
                if self.is_local(*id) {
                    return Ok(self.call(
                        self.universe_member("Ident"),
                        vec![self.type_name(&st.get(*id).name)],
                    ));
                }
                Ok(self.call(
                    self.support_member("mkTypeTree"),
                    vec![self.type_value(ty)?],
                ))
            }
            Type::ModuleRef(_) | Type::ThisType(_) | Type::SingleType { .. } => Ok(self.call(
                self.support_member("mkTypeTree"),
                vec![self.type_value(ty)?],
            )),
            Type::Applied { .. } => Ok(self.call(
                self.support_member("mkTypeTree"),
                vec![self.type_value(ty)?],
            )),
            other => Err(format!(
                "the type `{}` is not reified yet",
                st.display_type(other)
            )),
        }
    }

    fn applied_tree(&self, head: Tree, args: &[Type]) -> Result<Tree, String> {
        if args.is_empty() {
            return Ok(head);
        }
        let mut ts = Vec::new();
        for a in args {
            ts.push(self.type_tree_of_type(a)?);
        }
        Ok(self.call(
            self.universe_member("AppliedTypeTree"),
            vec![head, self.list(ts)],
        ))
    }

    /// The tree naming a class in a type position.
    fn class_type_tree(&self, cls: SymbolId) -> Result<Tree, String> {
        let st = self.st();
        let s = st.get(cls);
        if self.is_local(cls)
            || (s.kind == SymKind::Class
                && self.env().local_type_names.contains(&s.name)
                && !self.is_static_class(cls))
        {
            return Ok(self.call(self.universe_member("Ident"), vec![self.type_name(&s.name)]));
        }
        if let Some(enclosing) = self.enclosing_class_owner(cls) {
            let this = self.this_ref(enclosing, None)?;
            return Ok(self.call(
                self.universe_member("Select"),
                vec![this, self.type_name(&s.name)],
            ));
        }
        if let Some((home, alias)) = self.scala_alias(cls) {
            return Ok(self.call(
                self.universe_member("Select"),
                vec![self.static_module_ident(home), self.type_name(&alias)],
            ));
        }
        Ok(self.call(
            self.support_member("mkIdent"),
            vec![self.class_symbol(cls)?],
        ))
    }

    /// `List`, `Nil`, `Seq`, `Vector`, `::` ... are aliases in `scala`'s
    /// package object, and nsc's typer resolves a bare name to the alias:
    /// `Select(Ident(scala.package), TermName("List"))`. The prelude models
    /// the same alias by placing the symbol in package `scala` while its
    /// class file lives elsewhere; that mismatch is the alias.
    /// The object the alias lives in (`scala.package` or `scala.Predef`) and
    /// the alias's name.
    fn scala_alias(&self, sym: SymbolId) -> Option<(&'static str, String)> {
        let st = self.st();
        let s = st.get(sym);
        let name = s.name.trim_end_matches('$').to_string();
        if name.is_empty() {
            return None;
        }
        // `Map` and `Set` are `Predef`'s aliases, not the package object's.
        let home = if matches!(name.as_str(), "Map" | "Set") {
            "scala.Predef"
        } else {
            "scala.package"
        };
        let trimmed = |id: SymbolId| {
            let jvm = st.jvm_internal(id);
            jvm.strip_suffix('$').unwrap_or(&jvm).to_string()
        };
        let jvm = trimmed(sym);
        let direct = format!(
            "scala/{}",
            scala_rs_pickle::names::encode_method_name(&name)
        );
        // The prelude's alias: a symbol of that name placed in package
        // `scala` whose class file lives under another package. `sym` may
        // be that symbol, its module class (whose own `jvm_name` may be
        // unset), or the library's symbol for the same class file.
        let is_alias = st.lookup_member(st.scala_pkg, &name).into_iter().any(|c| {
            let cjvm = trimmed(c);
            cjvm != direct
                && cjvm.starts_with("scala/")
                && (c == sym || st.module_class_of(c) == sym || cjvm == jvm)
        });
        is_alias.then_some((home, name))
    }

    /// The class or object enclosing the `reify` that declares `cls`.
    fn enclosing_class_owner(&self, cls: SymbolId) -> Option<SymbolId> {
        let owner = self.st().get(cls).owner;
        self.env()
            .this_classes
            .iter()
            .copied()
            .find(|c| *c == owner)
    }

    /// `$m.staticClass("...")` or `rs.selectType(<static object>.asModule
    /// .moduleClass, "Name")`: the class symbol itself.
    fn class_symbol(&self, cls: SymbolId) -> Result<Tree, String> {
        let st = self.st();
        let s = st.get(cls);
        if s.kind != SymKind::Class {
            return Err(format!("`{}`, which is not a class", s.name));
        }
        if !self.is_static_class(cls) {
            return Err(format!(
                "`{}`, a class that is neither static nor defined inside the reify body",
                s.name
            ));
        }
        let owner = st.get(s.owner);
        match owner.kind {
            SymKind::Package => {
                if let Ok(full) = crate::materialize::static_class_of_sym(st, cls) {
                    return Ok(self.static_class(&full));
                }
                // A class read from a pickle keeps the package as its owner
                // even when it is nested in another class (`scala.reflect
                // .api.Exprs$Expr`): the class file's name says where it
                // lives, and `selectType` on that outer class finds it.
                let jvm = st.jvm_internal(cls);
                let (outer, inner) = jvm
                    .rsplit_once('$')
                    .filter(|(o, i)| !o.is_empty() && !i.is_empty() && !o.ends_with('/'))
                    .ok_or_else(|| {
                        format!("`{}`, a class nested in a class or an object", s.name)
                    })?;
                Ok(self.call(
                    self.support_member("selectType"),
                    vec![
                        self.static_class(&outer.replace('/', ".")),
                        self.lit(Lit::String(inner.to_string())),
                    ],
                ))
            }
            // `object O { class Inner }`: `selectType(O.moduleClass, "Inner")`.
            SymKind::ModuleClass | SymKind::Module => {
                let module = self.module_symbol(s.owner)?;
                Ok(self.call(
                    self.support_member("selectType"),
                    vec![
                        self.select(self.select(module, "asModule"), "moduleClass"),
                        self.lit(Lit::String(s.name.clone())),
                    ],
                ))
            }
            // `trait Exprs { class Expr }`: `selectType(staticClass("Exprs"),
            // "Expr")`. The prefix the type had (`universe.Expr`) is not
            // recorded, so its value form is `Exprs.this.Expr`.
            _ => {
                let outer = self.class_symbol(s.owner)?;
                Ok(self.call(
                    self.support_member("selectType"),
                    vec![outer, self.lit(Lit::String(s.name.clone()))],
                ))
            }
        }
    }

    /// A `$u.Type` value for `ty` -- what `setInfo`, `mkTypeTree` and a
    /// `TypeCreator` need (nsc's `reifyType`).
    pub(crate) fn type_value(&self, ty: &Type) -> Result<Tree, String> {
        let st = self.st();
        // `Foo.this.R`, `p.R`: an inner class whose prefix the typer kept as
        // a view (`crate::prefix`) is `TypeRef(<prefix>, R, args)`.
        if let Some(pre) = crate::prefix::view_prefix(ty) {
            let core = crate::prefix::strip_view(ty);
            if let Type::Class { sym, args } = core {
                if let Ok(prefix) = self.prefix_value(pre) {
                    let class = self.class_symbol(*sym)?;
                    let mut vs = Vec::new();
                    for a in args {
                        vs.push(self.type_value(a)?);
                    }
                    return Ok(self.call(
                        self.support_member("TypeRef"),
                        vec![prefix, class, self.list(vs)],
                    ));
                }
            }
            return self.type_value(core);
        }
        match ty {
            Type::Constant(l) => self.type_value(&Type::lit_underlying(l)),
            Type::AnyRef | Type::JavaObject => Ok(self.type_constructor(self.call(
                self.support_member("selectType"),
                vec![
                    self.package_class("scala"),
                    self.lit(Lit::String("AnyRef".into())),
                ],
            ))),
            // `Predef.String`, the alias, as nsc spells it:
            // `TypeRef(SingleType(SingleType(thisPrefix(RootClass),
            // staticPackage("scala")), staticModule("scala.Predef")),
            // selectType(Predef.moduleClass, "String"), Nil)`.
            Type::String => {
                let m = || self.local(&self.mirror_local());
                // `scala.type`, the prefix of `Predef`, is `TypeRef(ThisType
                // (<root>), scala, Nil)` in nsc's own tag for `String`.
                let scala = self.call(
                    self.support_member("TypeRef"),
                    vec![
                        self.call(
                            self.support_member("thisPrefix"),
                            vec![self.select(m(), "RootClass")],
                        ),
                        self.package_class("scala"),
                        self.list(vec![]),
                    ],
                );
                let predef = self.call(
                    self.support_member("SingleType"),
                    vec![
                        scala,
                        self.call(
                            self.select(m(), "staticModule"),
                            vec![self.lit(Lit::String("scala.Predef".into()))],
                        ),
                    ],
                );
                let alias = self.call(
                    self.support_member("selectType"),
                    vec![
                        self.module_class_of_static("scala.Predef"),
                        self.lit(Lit::String("String".into())),
                    ],
                );
                Ok(self.call(
                    self.support_member("TypeRef"),
                    vec![predef, alias, self.list(vec![])],
                ))
            }
            Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Char
            | Type::Int
            | Type::Long
            | Type::Float
            | Type::Double
            | Type::Unit
            | Type::Any
            | Type::AnyVal
            | Type::Nothing
            | Type::Null => {
                let name = crate::materialize::static_class_name(st, ty)?;
                Ok(self.type_constructor(self.static_class(&name)))
            }
            Type::Class { sym, args } => self.class_type_value(*sym, args),
            Type::Function { params, ret } => {
                let mut all: Vec<Type> = params.clone();
                all.push((**ret).clone());
                self.applied_value(
                    self.static_class(&format!("scala.Function{}", params.len())),
                    &all,
                )
            }
            Type::Tuple(xs) => {
                self.applied_value(self.static_class(&format!("scala.Tuple{}", xs.len())), xs)
            }
            Type::Array(elem) => {
                self.applied_value(self.static_class("scala.Array"), std::slice::from_ref(elem))
            }
            Type::TypeParam(id) | Type::TypeMember(id) => {
                if self.is_local(*id) {
                    return Err(format!(
                        "`{}`, a type parameter of a definition inside the reify body, in a position that needs a type value",
                        st.get(*id).name
                    ));
                }
                if let Some(ctx) = &self.reify {
                    if let Some(tag) = ctx.env.tags.get(id) {
                        return Ok(self.select(self.rebased(ctx, tag), "tpe"));
                    }
                }
                let local = self.free_type(*id)?;
                Ok(self.call(
                    self.support_member("TypeRef"),
                    vec![
                        self.universe_member("NoPrefix"),
                        self.local(&local),
                        self.list(vec![]),
                    ],
                ))
            }
            Type::Method { paramss, ret } if paramss.is_empty() => {
                let r = self.type_value(ret)?;
                Ok(self.call(self.support_member("NullaryMethodType"), vec![r]))
            }
            // `O.type`: `SingleType(ThisType(<O's owner>), <O>)`.
            Type::ModuleRef(mcls) => {
                let mcls = st.module_class_of(*mcls);
                let module = self.module_symbol(mcls)?;
                let owner = st.get(mcls).owner;
                let prefix = self.this_type_value(owner)?;
                Ok(self.call(self.support_member("SingleType"), vec![prefix, module]))
            }
            Type::ThisType(cls) => self.this_type_value(*cls),
            // `p.x.type` where `x` is an `object`: the same as its module
            // type. A stable `val`'s singleton is not built.
            Type::SingleType { sym, .. }
                if matches!(st.get(*sym).kind, SymKind::Module | SymKind::ModuleClass) =>
            {
                self.type_value(&Type::ModuleRef(*sym))
            }
            other => Err(format!(
                "the type `{}` is not reified yet",
                st.display_type(other)
            )),
        }
    }

    /// The module symbol of a static `object`: `$m.staticModule("O")` for a
    /// top-level one, `selectTerm(<outer>.moduleClass, "O")` for one nested
    /// in another static object.
    fn module_symbol(&self, mcls: SymbolId) -> Result<Tree, String> {
        let st = self.st();
        let mcls = st.module_class_of(mcls);
        if let Some(full) = self.static_module_full_name(mcls) {
            return Ok(self.call(
                self.select(self.local(&self.mirror_local()), "staticModule"),
                vec![self.lit(Lit::String(full))],
            ));
        }
        let s = st.get(mcls);
        let owner = st.get(s.owner);
        if matches!(owner.kind, SymKind::ModuleClass | SymKind::Module)
            && self.is_static_owner_chain(s.owner)
            && !self.is_local(mcls)
        {
            let outer = self.module_symbol(s.owner)?;
            return Ok(self.call(
                self.support_member("selectTerm"),
                vec![
                    self.select(self.select(outer, "asModule"), "moduleClass"),
                    self.lit(Lit::String(s.name.trim_end_matches('$').to_string())),
                ],
            ));
        }
        Err(format!(
            "`{}`, an `object` that is not static",
            s.name.trim_end_matches('$')
        ))
    }

    /// `scala.reflect.runtime.universe.type`, spelled as nsc's typer spells
    /// a path: `SingleType(SingleType(..., staticModule("scala.reflect
    /// .runtime.package")), selectTerm(<that module class>, "universe"))`.
    fn runtime_universe_singleton(&self) -> Tree {
        let m = || self.local(&self.mirror_local());
        let mut pre = self.call(
            self.support_member("thisPrefix"),
            vec![self.select(m(), "RootClass")],
        );
        for pkg in ["scala", "scala.reflect", "scala.reflect.runtime"] {
            pre = self.call(
                self.support_member("SingleType"),
                vec![
                    pre,
                    self.call(
                        self.select(m(), "staticPackage"),
                        vec![self.lit(Lit::String(pkg.into()))],
                    ),
                ],
            );
        }
        let pkg_object = self.call(
            self.select(m(), "staticModule"),
            vec![self.lit(Lit::String("scala.reflect.runtime.package".into()))],
        );
        let pre = self.call(self.support_member("SingleType"), vec![pre, pkg_object]);
        let universe = self.call(
            self.support_member("selectTerm"),
            vec![
                self.module_class_of_static("scala.reflect.runtime.package"),
                self.lit(Lit::String("universe".into())),
            ],
        );
        self.call(self.support_member("SingleType"), vec![pre, universe])
    }

    /// `ThisType(<package or static object>.asModule.moduleClass)`.
    fn this_type_value(&self, cls: SymbolId) -> Result<Tree, String> {
        let st = self.st();
        let s = st.get(cls);
        let module_class = match s.kind {
            SymKind::Package => self.package_class(&self.dotted_name(cls)),
            SymKind::ModuleClass | SymKind::Module => {
                let module = self.module_symbol(cls)?;
                self.select(self.select(module, "asModule"), "moduleClass")
            }
            SymKind::Class if self.is_static_class(cls) => self.class_symbol(cls)?,
            _ => {
                return Err(format!(
                    "`{}.this.type`, the type of a class instance, is not reified yet",
                    s.name
                ))
            }
        };
        Ok(self.call(self.support_member("ThisType"), vec![module_class]))
    }

    /// The value of a prefix a view carries: `ThisType(C)` for `C.this`,
    /// an `object`'s singleton for a stable module path.
    fn prefix_value(&self, pre: &Type) -> Result<Tree, String> {
        match pre {
            Type::ThisType(cls) => self.this_type_value(*cls),
            Type::ModuleRef(_) | Type::SingleType { .. } => self.type_value(pre),
            other => Err(format!(
                "a prefix of type `{}` is not reified yet",
                self.st().display_type(other)
            )),
        }
    }

    /// `<class>.asType.toTypeConstructor`.
    fn type_constructor(&self, class: Tree) -> Tree {
        self.select(self.select(class, "asType"), "toTypeConstructor")
    }

    /// `$u.appliedType(<class>, List(<args>))`, or the constructor alone.
    fn applied_value(&self, class: Tree, args: &[Type]) -> Result<Tree, String> {
        if args.is_empty() {
            return Ok(self.type_constructor(class));
        }
        let mut vs = Vec::new();
        for a in args {
            vs.push(self.type_value(a)?);
        }
        Ok(self.call(
            self.universe_member("appliedType"),
            vec![class, self.list(vs)],
        ))
    }

    /// A class at type arguments, as a type value.
    fn class_type_value(&self, cls: SymbolId, args: &[Type]) -> Result<Tree, String> {
        let st = self.st();
        let flat = st.dealias(&Type::Class {
            sym: cls,
            args: args.to_vec(),
        });
        match &flat {
            Type::Class { sym, args } if *sym == cls => {
                let class = self.class_symbol(*sym)?;
                let owner = st.get(*sym).owner;
                if st.get(owner).kind == SymKind::Class {
                    // A class nested in a class: `TypeRef(<prefix>, <class>,
                    // args)`, since `toTypeConstructor` would give the type
                    // seen from nowhere. For the reflection API's own cake
                    // (`Exprs.Expr`, `Trees.Tree`) the prefix that makes the
                    // type the one a program names is the runtime universe,
                    // `scala.reflect.runtime.universe.Expr[T]`; any other
                    // outer class gives `Outer.this`.
                    let outer = self.class_symbol(owner)?;
                    let prefix = if st.jvm_internal(owner).starts_with("scala/reflect/api/") {
                        self.runtime_universe_singleton()
                    } else {
                        self.call(self.support_member("ThisType"), vec![outer])
                    };
                    let mut vs = Vec::new();
                    for a in args {
                        vs.push(self.type_value(a)?);
                    }
                    return Ok(self.call(
                        self.support_member("TypeRef"),
                        vec![prefix, class, self.list(vs)],
                    ));
                }
                self.applied_value(class, args)
            }
            Type::Class { .. } => self.type_value(&flat),
            other if !matches!(other, Type::Class { .. }) => self.type_value(other),
            _ => unreachable!(),
        }
    }

    /// The type value for a type on its own -- the body of the
    /// `TypeCreator` the expansion's tag needs when the expression's type
    /// mentions a free type -- with the free symbols it needs bound in
    /// front, in a block. Uses and clears the symbol table.
    pub(crate) fn standalone_type_value(&self, ty: &Type) -> Result<Tree, String> {
        let _ = self.symtab.borrow_mut().take();
        let value = self.type_value(ty)?;
        let defs = self.symtab.borrow_mut().take();
        if defs.is_empty() {
            return Ok(value);
        }
        Ok(self.node(TreeKind::Block {
            stats: defs,
            expr: Box::new(value),
        }))
    }

    /// Whether `ty` mentions an abstract type that has neither a tag in scope
    /// nor a definition inside the body: the tag for the expression then
    /// needs a creator of its own (`Check::try_expand_reify`).
    pub(crate) fn needs_free_types(&self, ty: &Type) -> bool {
        let mut ids = Vec::new();
        collect_abstract(ty, &mut ids);
        let env = self.env();
        ids.iter()
            .any(|id| !env.local_syms.contains(id) && !env.tags.contains_key(id))
    }

    /// nsc's `reificationIsConcrete`: no free type, and every tag spliced in
    /// is a `TypeTag`. A concrete reification gets a `TypeTag`.
    pub(crate) fn is_concrete(&self, ty: &Type) -> bool {
        if self.needs_free_types(ty) {
            return false;
        }
        let mut ids = Vec::new();
        collect_abstract(ty, &mut ids);
        let env = self.env();
        ids.iter()
            .all(|id| env.local_syms.contains(id) || env.strong_tags.contains(id))
    }

    // -- patterns ------------------------------------------------------------

    /// The reify-mode reading of a pattern; `Ok(None)` defers to the
    /// quasiquote lowering, which builds the same trees by name.
    pub(super) fn reify_pat(&self, t: &Tree) -> Result<Option<Tree>, String> {
        if self.reify.is_none() {
            return Ok(None);
        }
        match &t.kind {
            // `x: T` binds `x`: `Bind(x, Typed(Ident(_), T))`.
            TreeKind::Typed { expr, tpt } => {
                let ty = self.typ(tpt)?;
                match &expr.kind {
                    TreeKind::Ident { name } if name != "_" && super::starts_lower(name) => {
                        let inner = self.call(
                            self.universe_member("Typed"),
                            vec![self.term_ident("_"), ty],
                        );
                        Ok(Some(self.call(
                            self.universe_member("Bind"),
                            vec![self.term_name(name), inner],
                        )))
                    }
                    TreeKind::Wildcard => Ok(Some(self.call(
                        self.universe_member("Typed"),
                        vec![self.term_ident("_"), ty],
                    ))),
                    TreeKind::Ident { name } if name == "_" => Ok(Some(self.call(
                        self.universe_member("Typed"),
                        vec![self.term_ident("_"), ty],
                    ))),
                    _ => {
                        let e = self.pat(expr)?;
                        Ok(Some(self.call(self.universe_member("Typed"), vec![e, ty])))
                    }
                }
            }
            // A stable identifier pattern (`Nil`, `None`) is a reference.
            TreeKind::Ident { name } if name != "_" && !super::starts_lower(name) => {
                if t.sym.is_none() {
                    return Ok(None);
                }
                Ok(Some(self.term_ref(t)?))
            }
            TreeKind::Ident { name } if name != "_" && t.stable_pat => Ok(Some(self.term_ref(t)?)),
            TreeKind::Star { elem } => {
                let e = self.pat(elem)?;
                Ok(Some(self.call(self.universe_member("Star"), vec![e])))
            }
            _ => Ok(None),
        }
    }

    /// The names a pattern binds, for the scope of its guard and body.
    pub(super) fn pattern_binders(&self, p: &Tree, out: &mut Vec<String>) {
        match &p.kind {
            TreeKind::Bind { name, body } => {
                out.push(name.clone());
                self.pattern_binders(body, out);
            }
            TreeKind::Ident { name }
                if name != "_" && super::starts_lower(name) && !p.stable_pat =>
            {
                out.push(name.clone());
            }
            TreeKind::Typed { expr, .. } => self.pattern_binders(expr, out),
            TreeKind::Apply { args, .. } | TreeKind::UnApply { args, .. } => {
                for a in args {
                    self.pattern_binders(a, out);
                }
            }
            TreeKind::Alternative { trees } => {
                for a in trees {
                    self.pattern_binders(a, out);
                }
            }
            TreeKind::Star { elem } => self.pattern_binders(elem, out),
            _ => {}
        }
    }

    // -- small pieces --------------------------------------------------------

    /// `$u.Ident($u.TermName("<name>"))`.
    fn plain_ident(&self, name: &str) -> Tree {
        self.call(self.universe_member("Ident"), vec![self.term_name(name)])
    }

    /// An untyped `Ident` of a name in the *expansion's* scope -- the value
    /// of a free term.
    fn plain_source_ident(&self, name: &str) -> Tree {
        self.node(TreeKind::Ident {
            name: name.to_string(),
        })
    }

    /// `$u.TypeTree()`.
    fn empty_type_tree(&self) -> Tree {
        self.call(
            self.select(self.support_member("SyntacticEmptyTypeTree"), "apply"),
            vec![],
        )
    }

    /// `while$1`, `doWhile$1`, `rassoc$1`: numbered per body.
    fn fresh_label(&self, prefix: &str) -> String {
        self.symtab.borrow_mut().fresh(prefix)
    }
}

/// Every abstract type (`TypeParam`, `TypeMember`) `ty` mentions.
pub(crate) fn collect_abstract(ty: &Type, out: &mut Vec<SymbolId>) {
    match ty {
        Type::TypeParam(id) | Type::TypeMember(id) => out.push(*id),
        Type::Class { args, .. } => args.iter().for_each(|a| collect_abstract(a, out)),
        Type::Function { params, ret } => {
            params.iter().for_each(|a| collect_abstract(a, out));
            collect_abstract(ret, out);
        }
        Type::Tuple(xs) => xs.iter().for_each(|a| collect_abstract(a, out)),
        Type::Array(e) | Type::ByName(e) | Type::Repeated(e) => collect_abstract(e, out),
        Type::Method { paramss, ret } => {
            paramss
                .iter()
                .flatten()
                .for_each(|a| collect_abstract(a, out));
            collect_abstract(ret, out);
        }
        Type::Applied { ctor, args } => {
            collect_abstract(ctor, out);
            args.iter().for_each(|a| collect_abstract(a, out));
        }
        Type::SingleType { prefix, .. } => collect_abstract(prefix, out),
        _ => {}
    }
}

fn kind_word(k: SymKind) -> &'static str {
    match k {
        SymKind::Module | SymKind::ModuleClass => "local `object`",
        SymKind::Class => "local class",
        SymKind::Package => "package",
        SymKind::Method => "method",
        SymKind::Term => "value",
        SymKind::TypeParam => "type parameter",
        SymKind::TypeMember => "type member",
        SymKind::NoSymbol => "symbol",
    }
}
