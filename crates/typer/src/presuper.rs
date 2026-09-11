//! The scope an early definition's right-hand side is typed in.
//!
//! nsc's `TreeGen.mkTemplate` moves the early section of
//! `class C(p: Int) extends { val x = e1; val y = e2 } with T` into the
//! primary constructor as *local* values in front of the super call, and
//! `typedPrimaryConstrBody` types that block in the constructor context
//! (`Context.makeConstructorContext`): the scope *outside* the template, plus
//! the constructor's parameters, plus the early values written before. So in
//!
//! ```scala
//! object O {
//!   val a = 5
//!   class D extends { val b = a; val c = b + 1 } with A { def a = 1 }
//! }
//! ```
//!
//! `a` is `O.a` (not the member `D.a`, which does not exist yet), `b` in `c`'s
//! right-hand side is the early value itself, and `this` is the enclosing
//! instance -- `O.this` -- exactly as in a super-constructor argument.
//! `type_ctor_delegation` builds the same context for `this(...)` arguments.

use crate::check::*;
use crate::symbol::Scope;
use scala_rs_parser::ast::*;

pub(crate) struct PresuperScope {
    scope: Option<(usize, Scope)>,
    this_class: SymbolId,
}

impl Typer {
    /// Enter the constructor context for the early value `val_sym`, or `None`
    /// when it is not an early definition of the class being typed.
    pub(crate) fn enter_presuper_scope(&mut self, val_sym: SymbolId) -> Option<PresuperScope> {
        if val_sym.is_none() {
            return None;
        }
        let class_id = self.st.get(val_sym).owner;
        if class_id.is_none() || !self.st.get(class_id).is_class_like() {
            return None;
        }
        let index = self
            .st
            .scopes
            .iter()
            .position(|scope| scope.template_owner == Some(class_id));
        let scope = index.map(|index| {
            let original = std::mem::take(&mut self.st.scopes[index]);
            let mut visible: Vec<SymbolId> = self.st.get(class_id).tparams.clone();
            let fields = self.st.get(class_id).ctor_fields.clone();
            visible.extend(fields.iter().copied());
            // The primary constructor's parameters: the fields plus any
            // evidence clause a context bound appended.
            if let Some(primary) = self.st.get(class_id).members.iter().copied().find(|&m| {
                let s = self.st.get(m);
                s.name == "<init>" && s.params.starts_with(&fields)
            }) {
                for p in self.st.get(primary).params.clone() {
                    if !visible.contains(&p) {
                        visible.push(p);
                    }
                }
            }
            // The early values written before this one; the rest are not
            // defined yet, as in any block.
            for m in self.st.get(class_id).members.clone() {
                if m == val_sym {
                    break;
                }
                let s = self.st.get(m);
                if s.kind == crate::symbol::SymKind::Term && s.flags.contains(Flags::PRESUPER) {
                    visible.push(m);
                }
            }
            for id in visible {
                let name = self.st.get(id).name.clone();
                self.st.scopes[index].enter(&name, id);
            }
            (index, original)
        });
        let mut outer = self.st.get(class_id).owner;
        while !outer.is_none() && !self.st.get(outer).is_class_like() {
            outer = self.st.get(outer).owner;
        }
        let this_class = std::mem::replace(&mut self.st.this_class, outer);
        Some(PresuperScope { scope, this_class })
    }

    pub(crate) fn leave_presuper_scope(&mut self, saved: PresuperScope) {
        if let Some((index, original)) = saved.scope {
            self.st.scopes[index] = original;
        }
        self.st.this_class = saved.this_class;
    }
}
