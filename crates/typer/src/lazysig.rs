//! On-demand completion of member signatures that have no type annotation.
//!
//! `val p = 1` / `def p = 1` only get a type once their right-hand side is
//! typed. The typer walks templates in source order, so a reference that is
//! reached *before* the definition has been typed would otherwise see
//! `<notype>`:
//!
//! ```scala
//! class C { def f: Int = D.p }
//! object D { val p = 1 }
//! ```
//!
//! nsc gives every symbol a lazy completer and runs it the moment the type is
//! needed. We do the same: the namer records the definition tree, the typer
//! refreshes that record with the scope the template was typed in, and a
//! reference to a still-uncompleted symbol runs the definition right there.
//! The completed tree is spliced back into the unit so nothing is typed twice
//! (signature side effects such as evidence parameters and default getters
//! must not be synthesized a second time).
//!
//! Re-entering a definition that is already being completed is nsc's
//! `CyclicReference`; we report it with nsc's wording
//! (`recursive value p needs type`, `recursive method f needs result type`).

use crate::check::Typer;
use crate::symbol::{Scope, SymKind};
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::rc::Rc;

/// A `name$default$n` body that still has to be typed.
///
/// The getter's *signature* is the parameter's type, so it is known as soon as
/// the method is; only the expression has to wait until every unit has been
/// walked. The scope stack of the definition is kept so the expression still
/// sees the imports and enclosing members it was written under.
pub(crate) struct PendingDefault {
    param: SymbolId,
    getter: SymbolId,
    ret: Type,
    tparams: Vec<SymbolId>,
    preceding: Vec<SymbolId>,
    owner: SymbolId,
    this_class: SymbolId,
    file_index: usize,
    scopes: Rc<Vec<Scope>>,
}

/// A definition whose signature still needs its right-hand side typed.
pub(crate) struct PendingSig {
    /// The definition tree. Typed at most once, then spliced back into the unit.
    pub tree: Tree,
    pub owner: SymbolId,
    pub this_class: SymbolId,
    /// Scope stack above the root scope, captured while the enclosing template
    /// was typed. `None` while only the namer has seen the definition; the
    /// scope is then rebuilt from the owner chain.
    pub scopes: Option<Rc<Vec<Scope>>>,
    /// `type_val_sig` / `type_def_sig` already ran on this tree.
    pub sig_done: bool,
}

/// `val x = rhs` / `def f = rhs`: no type annotation, but a body to infer from.
///
/// `type T = rhs` is here too: an alias is only usable once its right-hand side
/// has been resolved, and the units are typed in command-line order, so
/// `A.scala` may name `B.T` before `B.scala`'s template has been walked.
pub(crate) fn needs_lazy_sig(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::ValDef { tpt, rhs, .. } => tpt.is_empty() && !rhs.is_empty(),
        TreeKind::DefDef { tpt, rhs, name, .. } => {
            name != "<init>" && tpt.is_empty() && !rhs.is_empty()
        }
        TreeKind::TypeDef { rhs, .. } => !rhs.is_empty(),
        _ => false,
    }
}

impl Typer {
    /// Namer: remember the definition so a forward reference from another
    /// template can complete it. The scope is rebuilt from the owner chain,
    /// because the namer's scopes are not the ones the typer will use.
    pub(crate) fn register_namer_sig(&mut self, tree: &Tree) {
        if tree.sym.is_none() || !needs_lazy_sig(tree) {
            return;
        }
        if !self.st.get(self.st.owner).is_class_like() {
            return;
        }
        self.pending_sigs.insert(
            tree.sym,
            PendingSig {
                tree: tree.clone(),
                owner: self.st.owner,
                this_class: self.st.this_class,
                scopes: None,
                sig_done: false,
            },
        );
    }

    /// Typer signature pass: the definition still has no type, so keep it
    /// pending — now with the template's real scope (imports, inherited
    /// members, type aliases) and its already computed signature.
    pub(crate) fn register_typed_sig(&mut self, tree: &Tree) {
        if tree.sym.is_none() {
            return;
        }
        if !needs_lazy_sig(tree) {
            self.pending_sigs.remove(&tree.sym);
            return;
        }
        let scopes = Rc::new(self.st.scopes[self.lazy_base_scopes..].to_vec());
        self.pending_sigs.insert(
            tree.sym,
            PendingSig {
                tree: tree.clone(),
                owner: self.st.owner,
                this_class: self.st.this_class,
                scopes: Some(scopes),
                sig_done: true,
            },
        );
    }

    /// A definition that was completed on demand replaces the tree the unit
    /// still holds; both template passes then skip it.
    pub(crate) fn take_lazy_done(&mut self, tree: &mut Tree) -> bool {
        let sym = tree.sym;
        if sym.is_none() {
            return false;
        }
        if self.lazy_body_done.contains(&sym) {
            return true;
        }
        if let Some(done) = self.lazy_done.remove(&sym) {
            *tree = done;
            self.lazy_body_done.insert(sym);
            return true;
        }
        false
    }

    /// nsc's `LOCKED` flag: `type_def_body` types an unannotated method body
    /// with the method itself locked, so a reference back to it from a
    /// definition it completes reports the cycle at that reference.
    pub(crate) fn lock_lazy_sig(&mut self, sym: SymbolId) -> bool {
        if sym.is_none() || self.pending_sigs.remove(&sym).is_none() {
            return false;
        }
        self.lazy_completing.push(sym);
        true
    }

    pub(crate) fn unlock_lazy_sig(&mut self, locked: bool) {
        if locked {
            self.lazy_completing.pop();
        }
    }

    pub(crate) fn drop_lazy_sig(&mut self, sym: SymbolId) {
        if !sym.is_none() {
            self.pending_sigs.remove(&sym);
        }
    }

    /// Complete `id`'s signature if it is still pending. `span` is the
    /// reference that asked for the type; a cycle is reported there.
    pub(crate) fn complete_lazy_sig(&mut self, id: SymbolId, span: Span) {
        if id.is_none() {
            return;
        }
        if self.lazy_completing.contains(&id) {
            self.report_cyclic_sig(id, span);
            return;
        }
        let Some(p) = self.pending_sigs.remove(&id) else {
            return;
        };
        self.lazy_completing.push(id);
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        let saved_ret = self.return_meth;
        let saved_scopes = self.swap_in_pending_scopes(&p);
        self.st.owner = p.owner;
        self.st.this_class = p.this_class;
        self.return_meth = None;

        let mut t = p.tree;
        let is_val = matches!(t.kind, TreeKind::ValDef { .. });
        if matches!(t.kind, TreeKind::TypeDef { .. }) {
            self.complete_type_alias_tree(&mut t);
        } else {
            if !p.sig_done {
                if is_val {
                    self.type_val_sig(&mut t);
                } else {
                    self.type_def_sig(&mut t);
                }
            }
            if is_val {
                self.type_val_body(&mut t);
            } else {
                self.type_def_body(&mut t);
            }
        }

        self.swap_back_scopes(saved_scopes);
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.return_meth = saved_ret;
        self.lazy_completing.pop();
        self.lazy_done.insert(id, t);
    }

    /// Remember the body for `type_pending_defaults`, together with the scope
    /// stack it was written in.
    pub(crate) fn defer_default_getter_rhs(
        &mut self,
        param: SymbolId,
        getter: SymbolId,
        ret: &Type,
        tparams: &[SymbolId],
        preceding: &[SymbolId],
    ) {
        let scopes = Rc::new(self.st.scopes[self.lazy_base_scopes..].to_vec());
        self.pending_defaults.push(PendingDefault {
            param,
            getter,
            ret: ret.clone(),
            tparams: tparams.to_vec(),
            preceding: preceding.to_vec(),
            owner: self.st.owner,
            this_class: self.st.this_class,
            file_index: self.file_index,
            scopes,
        });
    }

    /// Type every deferred default body. Called once the signature pass has
    /// walked all units, so a default may name a member of any of them.
    pub(crate) fn type_pending_defaults(&mut self) {
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        let saved_file = self.file_index;
        // Typing one body can complete a signature that defines another
        // default, so drain until the queue stops growing.
        while !self.pending_defaults.is_empty() {
            for p in std::mem::take(&mut self.pending_defaults) {
                let saved_scopes = self.swap_in_scopes(Some(&p.scopes), p.owner);
                self.st.owner = p.owner;
                self.st.this_class = p.this_class;
                self.file_index = p.file_index;
                self.type_default_getter_rhs(p.param, p.getter, &p.ret, &p.tparams, &p.preceding);
                self.swap_back_scopes(saved_scopes);
            }
        }
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.file_index = saved_file;
    }

    /// nsc: `recursive value p needs type` / `recursive method f needs result type`.
    fn report_cyclic_sig(&mut self, id: SymbolId, span: Span) {
        if self.lazy_cyclic.insert(id) {
            let (kind, name) = {
                let s = self.st.get(id);
                (s.kind, s.name.clone())
            };
            let msg = if kind == SymKind::Method {
                format!("recursive method {name} needs result type")
            } else if kind == SymKind::TypeMember {
                format!("illegal cyclic reference involving type {name}")
            } else {
                format!("recursive value {name} needs type")
            };
            self.error(span, msg);
        }
        // nsc sets ErrorType on the locked symbol so the cycle unwinds with a
        // single message instead of a cascade of `<notype>` failures.
        self.pending_sigs.remove(&id);
        let ty = self.st.get(id).ty.clone();
        self.st.get_mut(id).ty = match ty {
            Type::Method { paramss, .. } => Type::Method {
                paramss,
                ret: Box::new(Type::Error),
            },
            _ => Type::Error,
        };
    }

    /// Install the scope stack the definition was named/typed in. The prelude
    /// scopes stay in place — they are never popped and cloning them would be
    /// expensive; only what the unit pushed on top is swapped out.
    fn swap_in_pending_scopes(&mut self, p: &PendingSig) -> Vec<Scope> {
        self.swap_in_scopes(p.scopes.as_ref(), p.owner)
    }

    fn swap_in_scopes(&mut self, scopes: Option<&Rc<Vec<Scope>>>, owner: SymbolId) -> Vec<Scope> {
        let mut saved = std::mem::take(&mut self.st.scopes);
        let base = self.lazy_base_scopes.min(saved.len());
        let mut stack: Vec<Scope> = saved.drain(..base).collect();
        match scopes {
            Some(sc) => stack.extend(sc.iter().cloned()),
            None => {
                let mut chain = Vec::new();
                let mut cur = owner;
                while !cur.is_none() && cur != self.st.root {
                    chain.push(cur);
                    let up = self.st.get(cur).owner;
                    if up == cur {
                        break;
                    }
                    cur = up;
                }
                chain.reverse();
                for c in chain {
                    let mut sc = Scope::default();
                    for m in self.st.get(c).members.clone() {
                        let n = self.st.get(m).name.clone();
                        sc.enter(&n, m);
                    }
                    stack.push(sc);
                }
            }
        }
        self.st.scopes = stack;
        saved
    }

    fn swap_back_scopes(&mut self, saved: Vec<Scope>) {
        let mut cur = std::mem::take(&mut self.st.scopes);
        cur.truncate(self.lazy_base_scopes.min(cur.len()));
        cur.extend(saved);
        self.st.scopes = cur;
    }
}
