//! Reification of *definitions*: `class`, `trait`, `object`, `def`, `val`.
//!
//! A child module of `crates/typer/src/reify.rs` (declared there with
//! `#[path]`), so it reaches the `Reifier`'s building blocks -- `call`,
//! `list`, `mods`, `support_member` -- without any of them having to be made
//! visible to the rest of the crate. Splitting the file this way keeps the
//! definition shapes off the lines the rest of reification occupies.
//!
//! Every shape was read off real scalac 2.13.16 with `-Ymacro-debug-lite`,
//! which prints what nsc's own quasiquote macro expands to:
//!
//! ```text
//! q"case class C(x: Int)"
//!   rs.SyntacticClassDef(
//!     u.Modifiers(rs.FlagsRepr(2048L), u.TypeName(""), List()),   // CASE
//!     u.TypeName("C"), List(), u.NoMods,
//!     List(List(rs.SyntacticValDef(
//!       u.Modifiers(rs.FlagsRepr(553648128L), ...),               // CASEACCESSOR | PARAMACCESSOR
//!       u.TermName("x"), rs.SyntacticTypeIdent(u.TypeName("Int")), u.EmptyTree))),
//!     List(),
//!     List(rs.ScalaDot(u.TypeName("Product")), rs.ScalaDot(u.TypeName("Serializable"))),
//!     u.noSelfType, List())
//! ```
//!
//! `tests/fixtures/dq_defs.scala` compares `showRaw` of each of them with real
//! scalac. As everywhere else in reification, a shape that cannot be rebuilt
//! faithfully is an error naming the form, never an approximation.

use super::Pos;
use scala_rs_parser::{Flags, Modifiers, Template, Tree, TreeKind};
use scala_rs_span::Span;

use super::Reifier;
use crate::quasiquote::hole_index;

/// The bits of `scala.reflect.internal.Flags` a `Modifiers` carries.
///
/// These are nsc's own numbering, which is *not* the parser's
/// (`scala_rs_parser::Flags`): `PRIVATE` is bit 2 there and bit 0 here, and so
/// on. Each value below was read back off `-Ymacro-debug-lite`, which prints
/// the `FlagsRepr(<n>L)` nsc builds -- `final class C` is `32L`, `case class`
/// `2048L`, `lazy val` `2147483648L`.
mod nsc {
    pub(super) const PROTECTED: i64 = 1 << 0;
    pub(super) const OVERRIDE: i64 = 1 << 1;
    pub(super) const PRIVATE: i64 = 1 << 2;
    pub(super) const ABSTRACT: i64 = 1 << 3;
    pub(super) const DEFERRED: i64 = 1 << 4;
    pub(super) const FINAL: i64 = 1 << 5;
    pub(super) const INTERFACE: i64 = 1 << 7;
    pub(super) const IMPLICIT: i64 = 1 << 9;
    pub(super) const SEALED: i64 = 1 << 10;
    pub(super) const CASE: i64 = 1 << 11;
    pub(super) const MUTABLE: i64 = 1 << 12;
    pub(super) const PARAM: i64 = 1 << 13;
    pub(super) const COVARIANT: i64 = 1 << 16;
    pub(super) const CONTRAVARIANT: i64 = 1 << 17;
    pub(super) const LOCAL: i64 = 1 << 19;
    pub(super) const CASEACCESSOR: i64 = 1 << 24;
    /// nsc reuses bit 25 for both, telling them apart by position.
    pub(super) const DEFAULTPARAM: i64 = 1 << 25;
    pub(super) const TRAIT: i64 = 1 << 25;
    pub(super) const PARAMACCESSOR: i64 = 1 << 29;
    pub(super) const LAZY: i64 = 1 << 31;
}

/// The modifiers each kind of definition accepts. Anything else is refused by
/// name rather than dropped.
const CLASS_MODS: Flags = Flags(
    Flags::PRIVATE.0
        | Flags::PROTECTED.0
        | Flags::ABSTRACT.0
        | Flags::FINAL.0
        | Flags::SEALED.0
        | Flags::IMPLICIT.0
        | Flags::CASE.0
        | Flags::TRAIT.0
        | Flags::LOCAL.0,
);
const OBJECT_MODS: Flags = Flags(
    Flags::PRIVATE.0
        | Flags::PROTECTED.0
        | Flags::FINAL.0
        | Flags::SEALED.0
        | Flags::IMPLICIT.0
        | Flags::CASE.0
        | Flags::LOCAL.0,
);
const DEF_MODS: Flags = Flags(
    Flags::PRIVATE.0
        | Flags::PROTECTED.0
        | Flags::OVERRIDE.0
        | Flags::ABSTRACT.0
        | Flags::FINAL.0
        | Flags::SEALED.0
        | Flags::IMPLICIT.0
        | Flags::LOCAL.0,
);
const VAL_MODS: Flags = Flags(DEF_MODS.0 | Flags::LAZY.0 | Flags::MUTABLE.0);
const CTOR_MODS: Flags = Flags(Flags::PRIVATE.0 | Flags::PROTECTED.0 | Flags::LOCAL.0);
const TPARAM_MODS: Flags = Flags(Flags::COVARIANT.0 | Flags::CONTRAVARIANT.0);
const PARAM_MODS: Flags = Flags(
    Flags::PARAM.0
        | Flags::IMPLICIT.0
        | Flags::DEFAULTPARAM.0
        | Flags::ACCESSOR.0
        | Flags::MUTABLE.0
        | Flags::PRIVATE.0
        | Flags::PROTECTED.0
        | Flags::OVERRIDE.0
        | Flags::LOCAL.0,
);

impl Reifier<'_> {
    /// One statement of a block or a template body: a definition, or an
    /// ordinary term.
    pub(super) fn definition(&self, t: &Tree) -> Result<Tree, String> {
        match &t.kind {
            TreeKind::ValDef { .. } => self.value_def(t),
            TreeKind::DefDef { .. } => self.method_def(t),
            TreeKind::ClassDef { .. } => self.class_def(t),
            TreeKind::ModuleDef { mods, name, impl_ } => self.object_def(t.span, mods, name, impl_),
            _ => self.term(t),
        }
    }

    /// `new C { ... }` -- the parser turns the anonymous class into a
    /// `ClassDef` named `$anon`, where nsc keeps the parents and the body in
    /// the `SyntacticNew` itself. `Ok(None)` when `t` is not one.
    pub(super) fn anon_new(&self, t: &Tree) -> Result<Option<Tree>, String> {
        let TreeKind::New { tpt } = &t.kind else {
            return Ok(None);
        };
        let TreeKind::ClassDef { name, impl_, .. } = &tpt.kind else {
            return Ok(None);
        };
        if name != "$anon" {
            return Ok(None);
        }
        let (parents, self_ty, body) = self.template(t.span, impl_, false)?;
        Ok(Some(self.call(
            self.support_member("SyntacticNew"),
            vec![self.list(vec![]), parents, self_ty, body],
        )))
    }

    // -- vals and vars -----------------------------------------------------

    /// `q"val v = e"`, `q"lazy val a = 1"`, `q"implicit val x: T = e"`,
    /// `q"var x = 1"`, and an abstract `q"val a: Int"` (which nsc marks
    /// `DEFERRED`).
    fn value_def(&self, t: &Tree) -> Result<Tree, String> {
        let TreeKind::ValDef {
            mods,
            name,
            tpt,
            rhs,
        } = &t.kind
        else {
            unreachable!("called for a ValDef");
        };
        if mods.flags.contains(Flags::PRESUPER) {
            return Err(
                "an early definition (`extends { val x = 1 } with T`) is not reified yet"
                    .to_string(),
            );
        }
        // `val (a, b) = e` is three definitions after parsing, and nsc keeps
        // it as one `SyntacticPatDef`; the trees are not the same.
        if name.starts_with("x$pat") {
            return Err("a pattern definition (`val (a, b) = ...`) is not reified yet".to_string());
        }
        let mut flags = self.def_flags(mods, VAL_MODS, "`val` definition")?;
        if rhs.is_empty() {
            flags |= nsc::DEFERRED;
        }
        let factory = if mods.flags.contains(Flags::MUTABLE) {
            "SyntacticVarDef"
        } else {
            "SyntacticValDef"
        };
        let r = self.rhs_expr(t.span, rhs)?;
        Ok(self.call(
            self.support_member(factory),
            vec![
                self.mods(flags),
                self.term_name_or_hole(name)?,
                self.type_or_empty(tpt)?,
                r,
            ],
        ))
    }

    /// A definition's right-hand side.
    ///
    /// `u.EmptyTree` when there is none, and `rs.SyntacticBlock(<xs>)` for
    /// `q"def f = {..$xs}"`: the parser folds `{ e }` down to `e`, so the only
    /// trace left of the author's braces is the source text -- the same
    /// reading `unwrap_body` needs for the quasiquote's outermost braces.
    fn rhs_expr(&self, span: Span, rhs: &Tree) -> Result<Tree, String> {
        if rhs.is_empty() {
            return Ok(self.universe_member("EmptyTree"));
        }
        if self.braced(span, rhs.span) {
            if let Some(t) = self.stats_splice(std::slice::from_ref(rhs))? {
                return Ok(t);
            }
        }
        self.term(rhs)
    }

    /// `u.Super(u.This(u.TypeName("<qual>")), u.TypeName("<mix>"))`.
    ///
    /// It lives here rather than in `reify.rs` because the only place `super`
    /// is written is the body of a definition (`override def g = super.g`),
    /// which is what slick's `ShapedValue.mapToImpl` does.
    pub(super) fn super_ref(&self, qual: Option<&str>, mix: Option<&str>) -> Tree {
        let this = self.call(
            self.universe_member("This"),
            vec![self.type_name(qual.unwrap_or(""))],
        );
        self.call(
            self.universe_member("Super"),
            vec![this, self.type_name(mix.unwrap_or(""))],
        )
    }

    // -- defs --------------------------------------------------------------

    /// `q"def f(...) = ..."`, with type parameters, several parameter
    /// clauses, and an implicit clause.
    fn method_def(&self, t: &Tree) -> Result<Tree, String> {
        let TreeKind::DefDef {
            mods,
            name,
            tparams,
            vparamss,
            tpt,
            rhs,
        } = &t.kind
        else {
            unreachable!("called for a DefDef");
        };
        let span = t.span;
        if name == "<init>" {
            return Err("an auxiliary constructor is not reified yet".to_string());
        }
        if matches!(rhs.kind, TreeKind::MacroRhs { .. }) {
            return Err("a `macro` definition is not reified yet".to_string());
        }
        // Procedure syntax (`def f() { ... }`) has no result type in the
        // source and nsc supplies `_root_.scala.Unit`; the parser leaves the
        // type empty, which would reify as `SyntacticEmptyTypeTree()`. The
        // `=` is what tells the two apart.
        if !rhs.is_empty() && tpt.is_empty() && !self.assigned(span, rhs.span) {
            return Err("procedure syntax (`def f() { ... }`) is not reified yet".to_string());
        }
        if rhs.is_empty() && tpt.is_empty() {
            return Err(
                "a `def` with neither a result type nor a body is not reified yet".to_string(),
            );
        }
        let mut flags = self.def_flags(mods, DEF_MODS, "`def` definition")?;
        if rhs.is_empty() {
            flags |= nsc::DEFERRED;
        }
        let r = self.rhs_expr(span, rhs)?;
        Ok(self.call(
            self.support_member("SyntacticDefDef"),
            vec![
                self.mods(flags),
                self.term_name_or_hole(name)?,
                self.type_params(tparams)?,
                self.param_clauses(vparamss, None)?,
                self.type_or_empty(tpt)?,
                r,
            ],
        ))
    }

    // -- classes, traits and objects ---------------------------------------

    /// `q"class C(...)"`, `q"case class C(...)"`, `q"trait T"`.
    fn class_def(&self, t: &Tree) -> Result<Tree, String> {
        let TreeKind::ClassDef {
            mods,
            name,
            tparams,
            ctor_mods,
            vparamss,
            impl_,
        } = &t.kind
        else {
            unreachable!("called for a ClassDef");
        };
        if name == "$anon" {
            return Err("an anonymous class is not reified yet".to_string());
        }
        let is_trait = mods.flags.contains(Flags::TRAIT);
        let what = if is_trait {
            "`trait` definition"
        } else {
            "class definition"
        };
        let is_case = mods.flags.contains(Flags::CASE);
        let flags = self.def_flags(mods, CLASS_MODS, what)?;
        let tps = self.type_params(tparams)?;
        let (parents, self_ty, body) = self.template(t.span, impl_, is_case)?;
        if is_trait {
            if !vparamss.is_empty() {
                return Err("a trait with parameters is not reified yet".to_string());
            }
            return Ok(self.call(
                self.support_member("SyntacticTraitDef"),
                vec![
                    self.mods(flags),
                    self.type_name_or_hole(name)?,
                    tps,
                    self.list(vec![]),
                    parents,
                    self_ty,
                    body,
                ],
            ));
        }
        let ctor = self.def_flags(ctor_mods, CTOR_MODS, "constructor")?;
        Ok(self.call(
            self.support_member("SyntacticClassDef"),
            vec![
                self.mods(flags),
                self.type_name_or_hole(name)?,
                tps,
                self.mods(ctor),
                self.param_clauses(vparamss, Some(is_case))?,
                self.list(vec![]),
                parents,
                self_ty,
                body,
            ],
        ))
    }

    /// `q"object O { ... }"`.
    fn object_def(
        &self,
        span: Span,
        mods: &Modifiers,
        name: &str,
        impl_: &Template,
    ) -> Result<Tree, String> {
        let is_case = mods.flags.contains(Flags::CASE);
        let flags = self.def_flags(mods, OBJECT_MODS, "object definition")?;
        let (parents, self_ty, body) = self.template(span, impl_, is_case)?;
        Ok(self.call(
            self.support_member("SyntacticObjectDef"),
            vec![
                self.mods(flags),
                self.term_name_or_hole(name)?,
                self.list(vec![]),
                parents,
                self_ty,
                body,
            ],
        ))
    }

    /// The parents, self type and body of a template.
    ///
    /// `span` is the definition's own, which is what tells `class C` (body
    /// `List()`) from `class C {}` (body `List(EmptyTree)`): the parser keeps
    /// no trace of braces around an empty body, but nsc builds different trees
    /// for the two, so the source text decides -- the same reading the arrow
    /// type and the block already need.
    fn template(
        &self,
        span: Span,
        impl_: &Template,
        is_case: bool,
    ) -> Result<(Tree, Tree, Tree), String> {
        if impl_.self_name.is_some() || impl_.self_tpt.is_some() {
            return Err("a self type (`class C { self => ... }`) is not reified yet".to_string());
        }
        let parents = match self.splice_clause(&impl_.parents, Pos::Type)? {
            Some(xs) => {
                if is_case {
                    return Err(
                        "a `case` class whose parents are a `..$` splice is not reified yet"
                            .to_string(),
                    );
                }
                xs
            }
            None => {
                let mut ps = Vec::new();
                for p in &impl_.parents {
                    ps.push(self.parent(p)?);
                }
                // nsc's parser supplies the parents the source leaves out:
                // `AnyRef` when there are none, and `Product with
                // Serializable` for every `case` class or object.
                if is_case {
                    ps.push(self.scala_dot("Product"));
                    ps.push(self.scala_dot("Serializable"));
                } else if ps.is_empty() {
                    ps.push(self.scala_dot("AnyRef"));
                }
                self.list(ps)
            }
        };
        for s in &impl_.body {
            if matches!(&s.kind, TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::PRESUPER))
            {
                return Err(
                    "an early definition (`extends { val x = 1 } with T`) is not reified yet"
                        .to_string(),
                );
            }
        }
        let body = if impl_.body.is_empty() {
            if self.text(span).trim_end().ends_with('}') {
                self.list(vec![self.universe_member("EmptyTree")])
            } else {
                self.list(vec![])
            }
        } else {
            match self.splice_clause(&impl_.body, Pos::Term)? {
                Some(xs) => xs,
                None => {
                    let mut out = Vec::new();
                    for s in &impl_.body {
                        out.push(self.definition(s)?);
                    }
                    self.list(out)
                }
            }
        };
        Ok((parents, self.universe_member("noSelfType"), body))
    }

    /// One parent: a type, wrapped in `SyntacticApplied` when the source gave
    /// the superclass constructor arguments (`extends D(1)`).
    fn parent(&self, p: &Tree) -> Result<Tree, String> {
        let mut clauses: Vec<&Vec<Tree>> = Vec::new();
        let mut cur = p;
        while let TreeKind::Apply { fun, args } = &cur.kind {
            clauses.push(args);
            cur = fun;
        }
        clauses.reverse();
        let mut head = self.typ(cur)?;
        if !clauses.is_empty() {
            let mut cs = Vec::new();
            for c in clauses {
                cs.push(self.arg_clause(c)?);
            }
            head = self.call(
                self.support_member("SyntacticApplied"),
                vec![head, self.list(cs)],
            );
        }
        Ok(head)
    }

    /// `rs.ScalaDot(u.TypeName("<name>"))` -- how nsc writes the parents it
    /// supplies itself, so they cannot be captured by a local `AnyRef`.
    fn scala_dot(&self, name: &str) -> Tree {
        self.call(self.support_member("ScalaDot"), vec![self.type_name(name)])
    }

    // -- parameters --------------------------------------------------------

    /// Every parameter clause.
    ///
    /// `class_case` is `None` for a `def` and `Some(is_case)` for a class,
    /// which changes the flags each parameter carries: a `def`'s are `PARAM`,
    /// a class's are `PARAMACCESSOR` plus `CASEACCESSOR` (first clause of a
    /// case class) or `PRIVATE | LOCAL` (no `val` / `var`).
    ///
    /// A trailing implicit clause is not one of the list: nsc passes it
    /// separately, as `rs.ImplicitParams(<the rest>, <the implicit clause>)`.
    fn param_clauses(
        &self,
        vparamss: &[Vec<Tree>],
        class_case: Option<bool>,
    ) -> Result<Tree, String> {
        let mut clauses = Vec::new();
        let mut implicits = None;
        for (i, c) in vparamss.iter().enumerate() {
            let is_implicit = c
                .first()
                .map(|p| param_mods(p).is_some_and(|m| m.flags.contains(Flags::IMPLICIT)))
                .unwrap_or(false);
            let built = self.param_clause(c, class_case.map(|case| case && i == 0))?;
            if is_implicit {
                if i + 1 != vparamss.len() {
                    return Err(
                        "an implicit parameter clause that is not the last is not reified yet"
                            .to_string(),
                    );
                }
                implicits = Some(built);
            } else {
                clauses.push(built);
            }
        }
        // A class always has a primary constructor, so a source with no
        // parameter list at all still reifies as one empty clause.
        if class_case.is_some() && clauses.is_empty() && implicits.is_none() {
            clauses.push(self.list(vec![]));
        }
        let rest = self.list(clauses);
        Ok(match implicits {
            Some(ic) => self.call(self.support_member("ImplicitParams"), vec![rest, ic]),
            None => rest,
        })
    }

    /// One parameter clause as a `List[Tree]`, or the `..$params` standing for
    /// the whole of it.
    fn param_clause(&self, c: &[Tree], class_case: Option<bool>) -> Result<Tree, String> {
        if let [only] = c {
            if let Some(i) = plain_param_hole(only) {
                if self.ranks.get(i).copied().unwrap_or(0) == 1 {
                    return self.hole(i, 1, Pos::Term);
                }
            }
        }
        let mut out = Vec::new();
        for p in c {
            out.push(self.param(p, class_case)?);
        }
        Ok(self.list(out))
    }

    fn param(&self, p: &Tree, class_case: Option<bool>) -> Result<Tree, String> {
        if let Some(i) = plain_param_hole(p) {
            return self.hole(i, 0, Pos::Term);
        }
        let TreeKind::ValDef {
            mods,
            name,
            tpt,
            rhs,
        } = &p.kind
        else {
            return Err("a parameter that is not a `val` is not reified yet".to_string());
        };
        if mods.flags.contains(Flags::BYNAME) {
            return Err("a by-name parameter is not reified yet".to_string());
        }
        if is_repeated(tpt) {
            return Err("a repeated parameter (`T*`) is not reified yet".to_string());
        }
        self.check_mods(mods, PARAM_MODS, "parameter")?;
        let f = mods.flags;
        let mut flags = 0i64;
        if f.contains(Flags::IMPLICIT) {
            flags |= nsc::IMPLICIT;
        }
        if f.contains(Flags::DEFAULTPARAM) {
            flags |= nsc::DEFAULTPARAM;
        }
        if f.contains(Flags::PRIVATE) {
            flags |= nsc::PRIVATE;
        }
        if f.contains(Flags::PROTECTED) {
            flags |= nsc::PROTECTED;
        }
        if f.contains(Flags::OVERRIDE) {
            flags |= nsc::OVERRIDE;
        }
        if f.contains(Flags::LOCAL) {
            flags |= nsc::LOCAL;
        }
        let mutable = f.contains(Flags::MUTABLE);
        match class_case {
            None => {
                if f.contains(Flags::ACCESSOR) || mutable {
                    return Err(
                        "a `val` / `var` parameter outside a class is not reified yet".to_string(),
                    );
                }
                flags |= nsc::PARAM;
            }
            Some(case_clause) => {
                flags |= nsc::PARAMACCESSOR;
                if mutable {
                    flags |= nsc::MUTABLE;
                }
                if case_clause {
                    flags |= nsc::CASEACCESSOR;
                } else if !f.contains(Flags::ACCESSOR) && !mutable {
                    // A plain `class C(x: Int)` parameter is not a member.
                    flags |= nsc::PRIVATE | nsc::LOCAL;
                }
            }
        }
        let factory = if mutable && class_case.is_some() {
            "SyntacticVarDef"
        } else {
            "SyntacticValDef"
        };
        let r = if rhs.is_empty() {
            self.universe_member("EmptyTree")
        } else {
            self.term(rhs)?
        };
        Ok(self.call(
            self.support_member(factory),
            vec![
                self.mods(flags),
                self.term_name_or_hole(name)?,
                self.type_or_empty(tpt)?,
                r,
            ],
        ))
    }

    // -- type parameters ---------------------------------------------------

    fn type_params(&self, tparams: &[Tree]) -> Result<Tree, String> {
        if let [only] = tparams {
            if let Some(i) = plain_tparam_hole(only) {
                if self.ranks.get(i).copied().unwrap_or(0) == 1 {
                    return self.hole(i, 1, Pos::Type);
                }
            }
        }
        let mut out = Vec::new();
        for tp in tparams {
            out.push(self.type_param(tp)?);
        }
        Ok(self.list(out))
    }

    /// `u.TypeDef(u.Modifiers(PARAM | <variance>), u.TypeName("T"), Nil,
    /// u.TypeBoundsTree(<lo>, <hi>))` -- not a `Syntactic*` call; nsc builds
    /// the `TypeDef` directly.
    fn type_param(&self, tp: &Tree) -> Result<Tree, String> {
        if let Some(i) = plain_tparam_hole(tp) {
            return self.hole(i, 0, Pos::Type);
        }
        let TreeKind::TypeDef {
            mods,
            name,
            tparams,
            rhs,
            lo,
            hi,
            views,
            ctx_bounds,
        } = &tp.kind
        else {
            return Err(
                "a type parameter that is not a type definition is not reified yet".to_string(),
            );
        };
        if !views.is_empty() {
            return Err("a view bound (`T <% U`) is not reified yet".to_string());
        }
        if !ctx_bounds.is_empty() {
            return Err("a context bound (`T : C`) is not reified yet".to_string());
        }
        if !tparams.is_empty() {
            return Err("a higher-kinded type parameter is not reified yet".to_string());
        }
        if !rhs.is_empty() {
            return Err("a type parameter with a right-hand side is not reified yet".to_string());
        }
        let mut flags = nsc::PARAM;
        self.check_mods(mods, TPARAM_MODS, "type parameter")?;
        if mods.flags.contains(Flags::COVARIANT) {
            flags |= nsc::COVARIANT;
        }
        if mods.flags.contains(Flags::CONTRAVARIANT) {
            flags |= nsc::CONTRAVARIANT;
        }
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
                self.mods(flags),
                self.type_name_or_hole(name)?,
                self.list(vec![]),
                bounds,
            ],
        ))
    }

    // -- modifiers ---------------------------------------------------------

    /// The nsc flag bits of a definition's modifiers.
    fn def_flags(&self, mods: &Modifiers, allowed: Flags, what: &str) -> Result<i64, String> {
        self.check_mods(mods, allowed, what)?;
        let f = mods.flags;
        let mut out = 0i64;
        for (flag, bits) in [
            (Flags::PRIVATE, nsc::PRIVATE),
            (Flags::PROTECTED, nsc::PROTECTED),
            (Flags::ABSTRACT, nsc::ABSTRACT),
            (Flags::FINAL, nsc::FINAL),
            (Flags::SEALED, nsc::SEALED),
            (Flags::IMPLICIT, nsc::IMPLICIT),
            (Flags::LAZY, nsc::LAZY),
            (Flags::OVERRIDE, nsc::OVERRIDE),
            (Flags::CASE, nsc::CASE),
            (Flags::MUTABLE, nsc::MUTABLE),
            (Flags::LOCAL, nsc::LOCAL),
            // nsc's parser spells `trait T` out as all three.
            (Flags::TRAIT, nsc::TRAIT | nsc::INTERFACE | nsc::ABSTRACT),
        ] {
            if f.contains(flag) {
                out |= bits;
            }
        }
        Ok(out)
    }

    /// Refuse the parts of a `Modifiers` reification does not carry over.
    fn check_mods(&self, mods: &Modifiers, allowed: Flags, what: &str) -> Result<(), String> {
        if !mods.annotations.is_empty() {
            return Err(format!("an annotated {what} is not reified yet"));
        }
        if mods.private_within.is_some() {
            return Err(
                "a qualified access modifier (`private[X]`) is not reified yet".to_string(),
            );
        }
        let extra = Flags(mods.flags.0 & !allowed.0);
        if extra.0 != 0 {
            return Err(format!(
                "the modifier `{}` on a {what} is not reified yet",
                modifier_name(extra)
            ));
        }
        Ok(())
    }

    // -- source text -------------------------------------------------------

    /// Whether the definition at `span` reaches its right-hand side through an
    /// `=`. Procedure syntax (`def f() { ... }`) does not, and nsc gives it a
    /// `Unit` result type the parser does not record.
    /// Whether the right-hand side of the definition at `span` was written
    /// inside braces the parser has since folded away (`{ e }` is `e`).
    fn braced(&self, span: Span, rhs: Span) -> bool {
        let (lo, hi) = (span.lo.to_usize(), rhs.lo.to_usize());
        match self.src.get(lo..hi) {
            Some(head) => head.trim_end().ends_with('{'),
            None => false,
        }
    }

    fn assigned(&self, span: Span, rhs: Span) -> bool {
        let (lo, hi) = (span.lo.to_usize(), rhs.lo.to_usize());
        let Some(head) = self.src.get(lo..hi) else {
            return false;
        };
        let head = head.trim_end();
        // A block right-hand side starts at its own `{`, which the parser
        // folds away when it holds a single expression.
        let head = head.strip_suffix('{').unwrap_or(head).trim_end();
        head.ends_with('=')
    }
}

/// The hole a `..$params` / `$param` placeholder parameter stands for.
///
/// It has to be a bare name: the parser reads `$p` in a parameter list as a
/// parameter of that name with no type, and anything more (`val $p: Int`)
/// would be a hole standing for something it is not.
fn plain_param_hole(p: &Tree) -> Option<usize> {
    let TreeKind::ValDef {
        mods,
        name,
        tpt,
        rhs,
    } = &p.kind
    else {
        return None;
    };
    if mods.flags != Flags::PARAM || !mods.annotations.is_empty() {
        return None;
    }
    if !tpt.is_empty() || !rhs.is_empty() {
        return None;
    }
    hole_index(name)
}

/// The same for a type parameter list: `class C[..$tparams]`.
fn plain_tparam_hole(tp: &Tree) -> Option<usize> {
    let TreeKind::TypeDef {
        mods,
        name,
        tparams,
        lo,
        hi,
        views,
        ctx_bounds,
        ..
    } = &tp.kind
    else {
        return None;
    };
    if mods.flags.0 != 0
        || !tparams.is_empty()
        || lo.is_some()
        || hi.is_some()
        || !views.is_empty()
        || !ctx_bounds.is_empty()
    {
        return None;
    }
    hole_index(name)
}

fn param_mods(p: &Tree) -> Option<&Modifiers> {
    match &p.kind {
        TreeKind::ValDef { mods, .. } => Some(mods),
        _ => None,
    }
}

/// `T*`, which the parser writes as `<repeated>[T]`.
fn is_repeated(tpt: &Tree) -> bool {
    matches!(&tpt.kind, TreeKind::AppliedTypeTree { tpt, .. }
        if matches!(&tpt.kind, TreeKind::Ident { name } if name == "<repeated>"))
}

/// The name of one modifier in `f`, for a diagnostic that says which.
fn modifier_name(f: Flags) -> &'static str {
    for (flag, name) in [
        (Flags::PRIVATE, "private"),
        (Flags::PROTECTED, "protected"),
        (Flags::ABSTRACT, "abstract"),
        (Flags::FINAL, "final"),
        (Flags::SEALED, "sealed"),
        (Flags::IMPLICIT, "implicit"),
        (Flags::LAZY, "lazy"),
        (Flags::OVERRIDE, "override"),
        (Flags::CASE, "case"),
        (Flags::TRAIT, "trait"),
        (Flags::MUTABLE, "var"),
        (Flags::BYNAME, "by-name"),
        (Flags::DEFAULTPARAM, "default argument"),
        (Flags::PRESUPER, "early definition"),
        (Flags::VOLATILE, "@volatile"),
        (Flags::TRANSIENT, "@transient"),
        (Flags::NATIVE, "@native"),
    ] {
        if f.contains(flag) {
            return name;
        }
    }
    "unknown"
}
