//! An unapplied method where a value is required and no function type is
//! expected: nsc's `adaptMethodTypeToExpr` once `isFunctionExpected` has
//! declined.
//!
//! ```scala
//! def add(a: Int, b: Int): Int = a + b
//! val f = add            // 2.13: missing argument list for method add in object Main
//! add.tupled((3, 4))     // 2.13: the same, at `add`
//! ```
//!
//! Scala 2.13 converts such a method to a function only when a function (or
//! SAM) type is expected, or with `add _`; anywhere else it is the error
//! above. `-Xsource:3` (`currentRun.isScala3`) eta-expands it wherever a
//! value is required, including as the prefix of a selection -- gitbucket's
//! `RepositoryOptions.apply.tupled`, written for Scala 3, depends on that.
//!
//! What the rule does *not* cover, each left to its own path:
//!
//! * a method whose first remaining clause is empty (`def f(): Int`) is
//!   auto-applied, with the deprecation nsc warns about;
//! * a first clause that is implicit is applied by implicit search, or is the
//!   missing implicit (`reject_unapplied_implicit_clause`);
//! * a constructor is never eta-expanded (`new C` is "not enough arguments");
//! * a macro cannot be eta-expanded at all (`reject_macro_eta`).
//!
//! scala-rs used to leave the method type standing, and `uncurry` turned
//! whatever reached it into a flattened function: `val f = add` compiled in
//! 2.13 mode, and `val c = curried` gave `c` the type of the *method*, so
//! that `c(2)(5)` read as `curried(2)` applied to `5`.

use crate::check::*;
use crate::symbol::SymKind;
use crate::uncurry::{eta_expand, eta_expand_curried};
use scala_rs_parser::ast::*;

impl Typer {
    /// The method `tree` names, when `tree` is a method value this rule
    /// governs: its type is a method type whose first remaining clause takes
    /// explicit parameters.
    pub(crate) fn unapplied_method_value(&self, tree: &Tree) -> Option<SymbolId> {
        let Type::Method { paramss, .. } = &tree.ty else {
            return None;
        };
        if paramss.first().is_none_or(|c| c.is_empty()) || tree.sym.is_none() {
            return None;
        }
        let s = self.st.get(tree.sym);
        if s.kind != SymKind::Method || s.name == "<init>" || s.flags.contains(Flags::CONSTRUCTOR) {
            return None;
        }
        // The clause standing first, among the declared ones: a partial
        // application (`curried(1)`) has consumed the clauses before it. A
        // method read from a class file may carry no parameter symbols at all
        // -- Java methods never have implicit parameters.
        if let Some(offset) = s.paramss.len().checked_sub(paramss.len()) {
            if s.paramss[offset]
                .iter()
                .any(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
            {
                return None;
            }
        }
        if self.macro_symbol_of(tree).is_some() {
            return None;
        }
        Some(tree.sym)
    }

    /// Whether `pt` is an expected type an unapplied method eta-expands
    /// against (nsc `isFunctionExpected`): a function type or a SAM, seen
    /// through a by-name or repeated formal and through aliases.
    pub(crate) fn expects_function_value(&self, pt: &Type) -> bool {
        let pt = match pt {
            Type::ByName(t) | Type::Repeated(t) => t.as_ref(),
            t => t,
        };
        let pt = self.st.dealias(pt);
        if matches!(pt, Type::Function { .. } | Type::Method { .. }) || is_function_pt(&pt) {
            return true;
        }
        if let Type::Class { sym, args } = &pt {
            if self.st.function_class_shape(*sym, args).is_some() {
                return true;
            }
            let jvm = &self.st.get(*sym).jvm_name;
            if jvm.starts_with("scala/Function") || jvm.ends_with("PartialFunction") {
                return true;
            }
        }
        self.st.sam_sig(&pt).is_some()
    }

    /// Apply the rule to `tree`, which is in value position with no function
    /// type expected. Returns whether it applied: in 2.13 mode `tree` is then
    /// an error, under `-Xsource:3` it is the eta-expansion.
    pub(crate) fn adapt_method_value(&mut self, tree: &mut Tree) -> bool {
        if matches!(tree.ty, Type::Overload(_)) && self.adapt_overloaded_value(tree, &Type::NoType)
        {
            return true;
        }
        let Some(meth) = self.unapplied_method_value(tree) else {
            return self.adapt_method_value_in_branches(tree);
        };
        if !self.scala3 {
            let msg = self.missing_argument_list_message(meth);
            self.error(tree.span, msg);
            tree.ty = Type::Error;
            return true;
        }
        self.eta_expand_method_value(tree);
        true
    }

    /// The same rule for the value positions *inside* a conditional, a match,
    /// a `try` or a block: nsc adapts each branch against the expected type,
    /// so `if (c) add else add` is two errors, not a method-typed `if`.
    /// Returns whether any branch changed; the tree's type is then rebuilt
    /// from the branches (an error if any branch is one).
    fn adapt_method_value_in_branches(&mut self, tree: &mut Tree) -> bool {
        let mut changed = false;
        let mut tys: Vec<Type> = Vec::new();
        match &mut tree.kind {
            TreeKind::If { thenp, elsep, .. } if !elsep.is_empty() => {
                changed |= self.adapt_method_value(thenp);
                changed |= self.adapt_method_value(elsep);
                tys.push(thenp.ty.clone());
                tys.push(elsep.ty.clone());
            }
            TreeKind::Match { cases, .. } => {
                for c in cases.iter_mut() {
                    changed |= self.adapt_method_value(&mut c.body);
                    tys.push(c.body.ty.clone());
                }
            }
            TreeKind::Try { block, catches, .. } => {
                changed |= self.adapt_method_value(block);
                tys.push(block.ty.clone());
                for c in catches.iter_mut() {
                    changed |= self.adapt_method_value(&mut c.body);
                    tys.push(c.body.ty.clone());
                }
            }
            TreeKind::Block { expr, .. } => {
                changed |= self.adapt_method_value(expr);
                tys.push(expr.ty.clone());
            }
            _ => {}
        }
        if !changed {
            return false;
        }
        tree.ty = if tys.iter().any(|t| t.is_error()) {
            Type::Error
        } else {
            let mut it = tys
                .into_iter()
                .filter(|t| !matches!(t, Type::Nothing) && !t.is_no_type());
            match it.next() {
                None => Type::Nothing,
                Some(first) => it.fold(first, |acc, t| self.lub_ty(&acc, &t)),
            }
        };
        true
    }

    /// Eta-expand the method reference `tree` (already typed) with `expand`,
    /// evaluating its receiver once, when the expansion is built.
    ///
    /// nsc's `etaExpand` lifts a prefix that is not safe to duplicate into a
    /// `val` in front of the function (`val eta$0 = mk(); (x) => eta$0.m(x)`).
    /// scala-rs put the receiver expression itself inside the lambda, so
    /// `val f: Int => Int = mk().m` ran `mk()` again on every call of `f` --
    /// a silent change of meaning, visible as soon as `mk` has an effect.
    pub(crate) fn eta_with_stable_receiver(
        &mut self,
        tree: &mut Tree,
        expand: impl FnOnce(&mut Self, &mut Tree),
    ) {
        let value = self.lift_eta_receiver(tree);
        expand(self, tree);
        if let Some(value) = value {
            let span = tree.span;
            let ty = tree.ty.clone();
            let expr = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
            let mut block = Tree::dummy(TreeKind::Block {
                stats: vec![value],
                expr: Box::new(expr),
            });
            block.span = span;
            block.ty = ty;
            *tree = block;
        }
    }

    /// Replace an impure receiver of the selection `tree` names with a fresh
    /// local, and return the typed `val` that binds it.
    fn lift_eta_receiver(&mut self, tree: &mut Tree) -> Option<Tree> {
        let selection = match &mut tree.kind {
            TreeKind::TypeApply { fun, .. } => fun.as_mut(),
            _ => tree,
        };
        let TreeKind::Select { qual, .. } = &mut selection.kind else {
            return None;
        };
        if self.eta_receiver_is_stable(qual) || qual.ty.is_no_type() || qual.ty.is_error() {
            return None;
        }
        let name = self.fresh("eta$receiver$");
        let ty = qual.ty.widen_constant();
        let span = qual.span;
        let sym = self.st.alloc(
            name.clone(),
            self.st.owner,
            SymKind::Term,
            Flags::FINAL.with(Flags::SYNTHETIC),
            "",
        );
        self.st.get_mut(sym).ty = ty.clone();
        let mut ident = Tree::dummy(TreeKind::Ident { name: name.clone() });
        ident.span = span;
        ident.sym = sym;
        ident.ty = ty.clone();
        let receiver = std::mem::replace(qual, Box::new(ident));
        let mut value = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::FINAL.with(Flags::SYNTHETIC)),
            name,
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: receiver,
        });
        value.span = span;
        value.sym = sym;
        value.ty = ty;
        Some(value)
    }

    /// nsc `treeInfo.isExprSafeToInline`, for an eta-expansion's prefix: a
    /// path of stable identifiers, `this`, `super`, a module, or a literal.
    fn eta_receiver_is_stable(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::This { .. } | TreeKind::Super { .. } | TreeKind::Literal { .. } => true,
            _ if matches!(t.ty, Type::ModuleRef(_)) => true,
            TreeKind::Ident { .. } | TreeKind::Select { .. } => {
                if t.sym.is_none() {
                    return false;
                }
                let s = self.st.get(t.sym);
                let stable = match s.kind {
                    SymKind::Module | SymKind::ModuleClass | SymKind::Package => true,
                    SymKind::Term => {
                        !s.flags.contains(Flags::MUTABLE) && !s.flags.contains(Flags::BYNAME)
                    }
                    _ => false,
                };
                stable
                    && match &t.kind {
                        TreeKind::Select { qual, .. } => self.eta_receiver_is_stable(qual),
                        _ => true,
                    }
            }
            _ => false,
        }
    }

    /// nsc `MissingArgsForMethodTpeError`, advice included.
    pub(crate) fn missing_argument_list_message(&self, meth: SymbolId) -> String {
        let s = self.st.get(meth);
        let name = s.name.clone();
        let owner = s.owner;
        // `Symbol.locationString`: only a class-like owner is named, so a
        // local method is "method local" and nothing more.
        let location = if !owner.is_none()
            && matches!(
                self.st.get(owner).kind,
                SymKind::Class | SymKind::Module | SymKind::ModuleClass
            ) {
            format!(" in {}", self.defining_owner_desc(owner))
        } else {
            String::new()
        };
        let clauses: Vec<usize> = if !s.pickle_clauses.is_empty() {
            s.pickle_clauses.clone()
        } else if !s.paramss.is_empty() {
            s.paramss.iter().map(|c| c.len()).collect()
        } else {
            match &s.ty {
                Type::Method { paramss, .. } => paramss.iter().map(|c| c.len()).collect(),
                _ => Vec::new(),
            }
        };
        let paf = format!(
            "{name}({})",
            clauses
                .iter()
                .map(|n| vec!["_"; *n].join(","))
                .collect::<Vec<_>>()
                .join(")(")
        );
        format!(
            "missing argument list for method {name}{location}\n\
             Unapplied methods are only converted to functions when a function type is expected.\n\
             You can make this conversion explicit by writing `{name} _` or `{paf}` instead of `{name}`."
        )
    }

    /// `-Xsource:3`: the eta-expansion nsc performs for a method value with no
    /// function type expected. One lambda per remaining explicit clause; a
    /// trailing implicit clause is filled inside the body. The method's own
    /// type parameters that nothing determined are `Nothing`, as nsc's
    /// `instantiate` leaves them against a wildcard (`val q = poly` is
    /// `Nothing => List[Nothing]`).
    fn eta_expand_method_value(&mut self, tree: &mut Tree) {
        if let Some(fn_ty) = self.implicit_eta_shape(tree) {
            let Type::Function { params, ret } = fn_ty.clone() else {
                return;
            };
            eta_expand(&mut self.st, &mut self.gensym, tree, params, *ret);
            self.type_expr(tree, &fn_ty);
            return;
        }
        let Type::Method { paramss, ret } = tree.ty.clone() else {
            return;
        };
        let meth = tree.sym;
        let open: Vec<SymbolId> = self
            .st
            .get(meth)
            .tparams
            .iter()
            .copied()
            .filter(|tp| {
                (paramss
                    .iter()
                    .flatten()
                    .any(|p| type_mentions_tparam(p, *tp))
                    || type_mentions_tparam(&ret, *tp))
                    && !self.tparam_in_scope(*tp)
            })
            .collect();
        let (paramss, ret) = if open.is_empty() {
            (paramss, *ret)
        } else {
            let nothing = vec![Type::Nothing; open.len()];
            let sub = |t: &Type| crate::symbol::subst_tparams_slice(&open, &nothing, t);
            (
                paramss
                    .iter()
                    .map(|c| c.iter().map(sub).collect())
                    .collect(),
                sub(&ret),
            )
        };
        self.eta_with_stable_receiver(tree, |this, tree| {
            eta_expand_curried(&mut this.st, &mut this.gensym, tree, &paramss, ret);
        });
    }
}
