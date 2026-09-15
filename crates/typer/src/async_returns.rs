//! Preserve non-local return identity across async-generated methods/classes.
//! A fresh key is allocated by each invocation of the lexical target method.
use crate::check::Typer;
use crate::lazy_local::children_mut;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

fn splice(mut t: Tree) -> Tree {
    t.id = NodeId::PRETYPED_SPLICE;
    t
}
fn type_tree(ty: Type) -> Tree {
    let mut t = Tree::dummy(TreeKind::Ident {
        name: crate::materialize::RESOLVED_TYPE.into(),
    });
    t.ty = ty;
    t
}
pub(crate) fn expression(source: &str) -> Tree {
    let parsed = scala_rs_parser::parse_str(&format!(
        "object AsyncReturnTemplate {{ val result = {{ {source} }} }}"
    ));
    assert!(
        !scala_rs_parser::has_errors(&parsed.diags),
        "internal async return template: {:?}",
        parsed.diags
    );
    let TreeKind::PackageDef { mut stats, .. } = parsed.tree.kind else {
        unreachable!()
    };
    let TreeKind::ModuleDef { mut impl_, .. } = stats.remove(0).kind else {
        unreachable!()
    };
    let TreeKind::ValDef { rhs, .. } = impl_.body.remove(0).kind else {
        unreachable!()
    };
    *rhs
}

impl Typer {
    pub(crate) fn return_template(
        &mut self,
        t: &mut Tree,
        span: Span,
        replacements: &[(&str, Tree)],
    ) {
        if let TreeKind::Ident { name } = &t.kind {
            if let Some((_, replacement)) = replacements.iter().find(|(n, _)| *n == name) {
                *t = replacement.clone();
                return;
            }
        }
        t.id = NodeId(self.macro_next_node);
        self.macro_next_node += 1;
        t.span = span;
        for child in children_mut(t) {
            self.return_template(child, span, replacements);
        }
    }

    pub(crate) fn prepare_async_returns(&mut self, t: &mut Tree) {
        // A return declared by a nested method keeps that method as its target.
        if matches!(
            t.kind,
            TreeKind::DefDef { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
        ) {
            return;
        }
        for child in children_mut(t) {
            self.prepare_async_returns(child);
        }
        let TreeKind::Return { expr } = &t.kind else {
            return;
        };
        if t.sym.is_none() {
            return;
        }
        let target = t.sym;
        let span = t.span;
        if !self.async_return_keys.contains_key(&target) {
            let name = self.fresh("async$returnKey");
            let mut rhs = expression("new _root_.java.lang.Object()");
            self.return_template(&mut rhs, span, &[]);
            self.type_expr(&mut rhs, &Type::NoType);
            let sym = self
                .st
                .alloc(&name, target, SymKind::Term, Flags::SYNTHETIC, "");
            self.st.get_mut(sym).ty = rhs.ty.clone();
            let mut reference = Tree::dummy(TreeKind::Ident { name: name.clone() });
            reference.sym = sym;
            reference.ty = rhs.ty.clone();
            let mut def = Tree::dummy(TreeKind::ValDef {
                mods: Modifiers::new(Flags::SYNTHETIC),
                name,
                tpt: Box::new(type_tree(rhs.ty.clone())),
                rhs: Box::new(rhs),
            });
            def.sym = sym;
            def.ty = reference.ty.clone();
            def.span = span;
            self.async_return_keys
                .insert(target, (def, splice(reference)));
        }
        let key = self.async_return_keys[&target].1.clone();
        let mut replacement = expression(
            "throw new _root_.scala.runtime.NonLocalReturnControl[Any](AsyncKey, AsyncValue)",
        );
        self.return_template(
            &mut replacement,
            span,
            &[("AsyncKey", key), ("AsyncValue", splice((**expr).clone()))],
        );
        // The original operand keeps its attribution, including await markers;
        // continuation lowering will subsequently consume those markers.
        self.type_expr(&mut replacement, &Type::NoType);
        *t = replacement;
    }

    pub(crate) fn install_async_return_keys(&mut self, t: &mut Tree) {
        if self.async_return_keys.is_empty() {
            return;
        }
        for child in children_mut(t) {
            self.install_async_return_keys(child);
        }
        let TreeKind::DefDef { rhs, .. } = &mut t.kind else {
            return;
        };
        let Some((key_def, key_ref)) = self.async_return_keys.remove(&t.sym) else {
            return;
        };
        let result_ty = match &self.st.get(t.sym).ty {
            Type::Method { ret, .. } => (**ret).clone(),
            _ => rhs.ty.clone(),
        };
        let mut caught = expression("try AsyncBody catch { case ex: _root_.scala.runtime.NonLocalReturnControl[_] if ex.key eq AsyncKey => ex.value.asInstanceOf[AsyncResult] }");
        self.return_template(
            &mut caught,
            t.span,
            &[
                ("AsyncBody", splice((**rhs).clone())),
                ("AsyncKey", key_ref),
                ("AsyncResult", type_tree(result_ty.clone())),
            ],
        );
        let saved = self.st.owner;
        self.st.owner = t.sym;
        self.type_expr(&mut caught, &result_ty);
        self.st.owner = saved;
        let mut block = Tree::dummy(TreeKind::Block {
            stats: vec![key_def],
            expr: Box::new(caught),
        });
        block.ty = result_ty;
        block.span = t.span;
        **rhs = block;
    }
}
