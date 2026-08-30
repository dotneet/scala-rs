//! Lambda-lift nested `def`s that capture locals onto the enclosing class.
//!
//! Runs after uncurry and before erasure. Nested methods become synthetic
//! class members with extra parameters for captured locals; call sites
//! (including eta-expanded functions and recursive calls from lambdas) pass
//! those captures so the backend actually emits and runs them.

use std::collections::{HashMap, HashSet};

use scala_rs_parser::{Flags, Modifiers, NodeId, SymbolId, Tree, TreeKind, Type};

use crate::symbol::{SymKind, SymbolTable};

/// Rewrite `tree` in place: hoist nested defs and thread captured locals.
pub fn lambda_lift(tree: &mut Tree, st: &mut SymbolTable) {
    // The driver runs `mark_anon_captures` *after* this pass (it wants the
    // already-lifted tree shape for the final, backend-facing answer). But a
    // nested `def` that constructs a local class needing its own captured
    // locals (`class F(...) { def m = ... factor ... }; def helper() = new
    // F(x)`) has to receive `factor` too -- `collect_captures`'s `New` arm
    // below reads it off `Symbol::captures`, which is otherwise still empty
    // at this point. Seed it early from the pre-lift tree, which is exactly
    // the shape `mark_anon_captures`'s own walk expects; the driver's later
    // call recomputes it from the lifted tree and simply overwrites this with
    // the same (or, for classes touched by lifting, the final) answer.
    crate::anon_capture::mark_anon_captures(tree, st);
    let mut l = Lifter { st, gensym: 0 };
    l.lift_tree(tree);
}

struct Lifter<'a> {
    st: &'a mut SymbolTable,
    gensym: u32,
}

impl<'a> Lifter<'a> {
    fn lift_tree(&mut self, tree: &mut Tree) {
        match &mut tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.lift_tree(s);
                }
            }
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => self.lift_template(tree),
            _ => {}
        }
    }

    fn lift_template(&mut self, tree: &mut Tree) {
        let class_id = tree.sym;
        self.lift_nested_classes(tree);
        let mut nested = Vec::new();
        match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.extract_from(p, &mut nested);
                    }
                }
                for s in &mut impl_.body {
                    if matches!(
                        s.kind,
                        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
                    ) {
                        continue;
                    }
                    self.extract_from(s, &mut nested);
                }
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for s in &mut impl_.body {
                    if matches!(
                        s.kind,
                        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
                    ) {
                        continue;
                    }
                    self.extract_from(s, &mut nested);
                }
            }
            _ => {}
        }
        let mut cap_map: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
        for d in &nested {
            if d.sym.is_none() {
                continue;
            }
            cap_map.insert(d.sym, self.captures(d));
        }
        // A def that *calls* another hoisted def has to be able to hand it its
        // captures, and `extract_from` has already pulled the callee out of the
        // caller's body, so nothing in the caller mentions them any more:
        //
        //     def f(n: Int) = { def inner(m: Int) = { def g = m + n; g + g }; … }
        //
        // `g` captures `m` and `n`; `inner` reads neither by itself, yet the
        // rewritten `g(m, n)` inside it needs both. Only `m` is `inner`'s own,
        // so `n` has to reach `inner` as a capture too. Without this the call
        // to `inner` was emitted one argument short and the method ran on a
        // shifted frame.
        close_over_callees(&nested, &mut cap_map, self.st);
        for d in &mut nested {
            if d.sym.is_none() {
                continue;
            }
            let caps = cap_map.get(&d.sym).cloned().unwrap_or_default();
            self.reparent(d, class_id, &caps);
        }
        match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        rewrite_calls(p, &cap_map, self.st);
                    }
                }
                for s in &mut impl_.body {
                    rewrite_calls(s, &cap_map, self.st);
                }
                for d in &mut nested {
                    rewrite_calls(d, &cap_map, self.st);
                }
                impl_.body.extend(nested);
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for s in &mut impl_.body {
                    rewrite_calls(s, &cap_map, self.st);
                }
                for d in &mut nested {
                    rewrite_calls(d, &cap_map, self.st);
                }
                impl_.body.extend(nested);
            }
            _ => {}
        }
    }

    fn lift_nested_classes(&mut self, tree: &mut Tree) {
        match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.lift_nested_classes(p);
                    }
                }
                for p in &mut impl_.parents {
                    self.lift_nested_classes(p);
                }
                for s in &mut impl_.body {
                    if matches!(
                        s.kind,
                        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
                    ) {
                        self.lift_template(s);
                    } else {
                        self.lift_nested_classes(s);
                    }
                }
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for p in &mut impl_.parents {
                    self.lift_nested_classes(p);
                }
                for s in &mut impl_.body {
                    if matches!(
                        s.kind,
                        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
                    ) {
                        self.lift_template(s);
                    } else {
                        self.lift_nested_classes(s);
                    }
                }
            }
            TreeKind::New { tpt } => {
                if matches!(tpt.kind, TreeKind::ClassDef { .. }) {
                    self.lift_template(tpt);
                } else {
                    self.lift_nested_classes(tpt);
                }
            }
            TreeKind::ValDef { tpt, rhs, .. } => {
                self.lift_nested_classes(tpt);
                self.lift_nested_classes(rhs);
            }
            TreeKind::DefDef {
                vparamss, tpt, rhs, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.lift_nested_classes(p);
                    }
                }
                self.lift_nested_classes(tpt);
                self.lift_nested_classes(rhs);
            }
            TreeKind::Block { stats, expr } => {
                for s in stats {
                    self.lift_nested_classes(s);
                }
                self.lift_nested_classes(expr);
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.lift_nested_classes(cond);
                self.lift_nested_classes(thenp);
                self.lift_nested_classes(elsep);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                self.lift_nested_classes(cond);
                self.lift_nested_classes(body);
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.lift_nested_classes(p);
                }
                self.lift_nested_classes(body);
            }
            TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
                self.lift_nested_classes(fun);
                for a in args {
                    self.lift_nested_classes(a);
                }
            }
            TreeKind::Typed { expr, tpt } => {
                self.lift_nested_classes(expr);
                self.lift_nested_classes(tpt);
            }
            TreeKind::Select { qual, .. } => self.lift_nested_classes(qual),
            TreeKind::Match { selector, cases } => {
                self.lift_nested_classes(selector);
                for c in cases {
                    self.lift_nested_classes(&mut c.pat);
                    self.lift_nested_classes(&mut c.guard);
                    self.lift_nested_classes(&mut c.body);
                }
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.lift_nested_classes(block);
                for c in catches {
                    self.lift_nested_classes(&mut c.pat);
                    self.lift_nested_classes(&mut c.body);
                }
                self.lift_nested_classes(finalizer);
            }
            TreeKind::Assign { lhs, rhs } => {
                self.lift_nested_classes(lhs);
                self.lift_nested_classes(rhs);
            }
            TreeKind::Return { expr } | TreeKind::Throw { expr } => self.lift_nested_classes(expr),
            _ => {}
        }
    }

    fn extract_from(&mut self, tree: &mut Tree, out: &mut Vec<Tree>) {
        match &mut tree.kind {
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {}
            TreeKind::New { tpt } if matches!(tpt.kind, TreeKind::ClassDef { .. }) => {}
            TreeKind::Block { stats, expr } => {
                for s in stats {
                    if matches!(s.kind, TreeKind::DefDef { .. }) {
                        let mut d = std::mem::replace(s, Tree::dummy(TreeKind::Empty));
                        if let TreeKind::DefDef { rhs, .. } = &mut d.kind {
                            self.extract_from(rhs, out);
                        }
                        out.push(d);
                    } else {
                        self.extract_from(s, out);
                    }
                }
                if matches!(expr.kind, TreeKind::DefDef { .. }) {
                    let mut d = std::mem::replace(expr, Box::new(Tree::dummy(TreeKind::Empty)));
                    if let TreeKind::DefDef { rhs, .. } = &mut d.kind {
                        self.extract_from(rhs, out);
                    }
                    out.push(*d);
                } else {
                    self.extract_from(expr, out);
                }
            }
            TreeKind::DefDef {
                vparamss, tpt, rhs, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.extract_from(p, out);
                    }
                }
                self.extract_from(tpt, out);
                self.extract_from(rhs, out);
            }
            TreeKind::ValDef { tpt, rhs, .. } => {
                self.extract_from(tpt, out);
                self.extract_from(rhs, out);
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.extract_from(p, out);
                }
                self.extract_from(body, out);
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.extract_from(cond, out);
                self.extract_from(thenp, out);
                self.extract_from(elsep, out);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                self.extract_from(cond, out);
                self.extract_from(body, out);
            }
            TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
                self.extract_from(fun, out);
                for a in args {
                    self.extract_from(a, out);
                }
            }
            TreeKind::Typed { expr, tpt } => {
                self.extract_from(expr, out);
                self.extract_from(tpt, out);
            }
            TreeKind::Select { qual, .. } => self.extract_from(qual, out),
            TreeKind::Match { selector, cases } => {
                self.extract_from(selector, out);
                for c in cases {
                    self.extract_from(&mut c.pat, out);
                    self.extract_from(&mut c.guard, out);
                    self.extract_from(&mut c.body, out);
                }
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.extract_from(block, out);
                for c in catches {
                    self.extract_from(&mut c.pat, out);
                    self.extract_from(&mut c.body, out);
                }
                self.extract_from(finalizer, out);
            }
            TreeKind::Assign { lhs, rhs } => {
                self.extract_from(lhs, out);
                self.extract_from(rhs, out);
            }
            TreeKind::Return { expr } | TreeKind::Throw { expr } => self.extract_from(expr, out),
            TreeKind::LabelDef { params, rhs, .. } => {
                for p in params {
                    self.extract_from(p, out);
                }
                self.extract_from(rhs, out);
            }
            _ => {}
        }
    }

    fn captures(&self, def: &Tree) -> Vec<SymbolId> {
        let mut own = HashSet::new();
        own.insert(def.sym);
        if let TreeKind::DefDef { vparamss, .. } = &def.kind {
            for p in vparamss.iter().flatten() {
                if !p.sym.is_none() {
                    own.insert(p.sym);
                }
            }
        }
        for p in &self.st.get(def.sym).params {
            own.insert(*p);
        }
        let mut out = Vec::new();
        collect_captures(def, &own, &mut out, self.st);
        out
    }

    fn reparent(&mut self, def: &mut Tree, class_id: SymbolId, caps: &[SymbolId]) {
        if def.sym.is_none() || class_id.is_none() {
            return;
        }
        let id = def.sym;
        self.st.get_mut(id).owner = class_id;
        if !self.st.get(class_id).members.contains(&id) {
            self.st.get_mut(class_id).members.push(id);
        }
        self.gensym += 1;
        let orig = self.st.get(id).name.clone();
        let new_name = format!("{}${}", orig, self.gensym);
        self.st.get_mut(id).name = new_name.clone();
        let flags = self.st.get(id).flags.with(Flags::SYNTHETIC);
        self.st.get_mut(id).flags = flags;

        let cap_tys: Vec<Type> = caps.iter().map(|c| self.st.get(*c).ty.clone()).collect();
        let cap_names: Vec<String> = caps.iter().map(|c| self.st.get(*c).name.clone()).collect();

        if let TreeKind::DefDef {
            name,
            mods,
            vparamss,
            ..
        } = &mut def.kind
        {
            *name = new_name;
            mods.flags = mods.flags.with(Flags::SYNTHETIC);
            let mut cap_vals = Vec::new();
            for (i, &cid) in caps.iter().enumerate() {
                let mut vd = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(Flags::PARAM),
                    name: cap_names[i].clone(),
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                vd.span = def.span;
                vd.sym = cid;
                vd.ty = cap_tys[i].clone();
                cap_vals.push(vd);
            }
            if vparamss.is_empty() {
                if !cap_vals.is_empty() {
                    vparamss.push(cap_vals);
                }
            } else {
                vparamss[0].splice(0..0, cap_vals);
            }
        }

        match &mut def.ty {
            Type::Method { paramss, .. } => {
                if paramss.is_empty() {
                    if !cap_tys.is_empty() {
                        paramss.push(cap_tys);
                    }
                } else {
                    paramss[0].splice(0..0, cap_tys);
                }
            }
            _ => {
                if !cap_tys.is_empty() {
                    def.ty = Type::Method {
                        paramss: vec![cap_tys],
                        ret: Box::new(def.ty.clone()),
                    };
                }
            }
        }
        self.st.get_mut(id).ty = def.ty.clone();
        let mut params = caps.to_vec();
        params.extend(self.st.get(id).params.clone());
        self.st.get_mut(id).params = params.clone();
        let mut paramss = self.st.get(id).paramss.clone();
        if paramss.is_empty() {
            if !params.is_empty() {
                self.st.get_mut(id).paramss = vec![params];
            }
        } else {
            paramss[0].splice(0..0, caps.iter().copied());
            self.st.get_mut(id).paramss = paramss;
        }
    }
}

/// Add to each hoisted def the captures of every hoisted def it calls, minus
/// whatever it defines itself. Repeated to a fixpoint, so a chain of three
/// nested defs threads the outermost local all the way down.
fn close_over_callees(
    nested: &[Tree],
    cap_map: &mut HashMap<SymbolId, Vec<SymbolId>>,
    st: &SymbolTable,
) {
    let known: HashSet<SymbolId> = cap_map.keys().copied().collect();
    if known.len() < 2 {
        return;
    }
    let mut bound: HashMap<SymbolId, HashSet<SymbolId>> = HashMap::new();
    let mut callees: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
    for d in nested {
        if d.sym.is_none() {
            continue;
        }
        let mut b = HashSet::new();
        b.insert(d.sym);
        for p in &st.get(d.sym).params {
            b.insert(*p);
        }
        collect_bound(d, &mut b);
        bound.insert(d.sym, b);
        let mut c = Vec::new();
        collect_callees(d, &known, &mut c);
        callees.insert(d.sym, c);
    }
    loop {
        let mut changed = false;
        for d in nested {
            if d.sym.is_none() {
                continue;
            }
            let mut add: Vec<SymbolId> = Vec::new();
            for callee in callees.get(&d.sym).into_iter().flatten() {
                if *callee == d.sym {
                    continue;
                }
                for c in cap_map.get(callee).into_iter().flatten() {
                    if bound[&d.sym].contains(c) || cap_map[&d.sym].contains(c) || add.contains(c) {
                        continue;
                    }
                    add.push(*c);
                }
            }
            if !add.is_empty() {
                changed = true;
                cap_map.get_mut(&d.sym).expect("cap_map").extend(add);
            }
        }
        if !changed {
            return;
        }
    }
}

/// Every symbol `tree` introduces below its own head: locals, nested defs and
/// classes, lambda and pattern binders. Anything in here is in scope at a call
/// site inside `tree`, so it is never a capture of it.
fn collect_bound(tree: &Tree, out: &mut HashSet<SymbolId>) {
    match &tree.kind {
        TreeKind::ValDef { .. }
        | TreeKind::DefDef { .. }
        | TreeKind::ClassDef { .. }
        | TreeKind::ModuleDef { .. }
        | TreeKind::Bind { .. }
        | TreeKind::LabelDef { .. }
            if !tree.sym.is_none() =>
        {
            out.insert(tree.sym);
        }
        _ => {}
    }
    for c in child_trees(tree) {
        collect_bound(c, out);
    }
}

/// Hoisted defs named anywhere below `tree`.
fn collect_callees(tree: &Tree, known: &HashSet<SymbolId>, out: &mut Vec<SymbolId>) {
    if !tree.sym.is_none() && known.contains(&tree.sym) && !out.contains(&tree.sym) {
        out.push(tree.sym);
    }
    for c in child_trees(tree) {
        collect_callees(c, known, out);
    }
}

fn child_trees(t: &Tree) -> Vec<&Tree> {
    let mut v: Vec<&Tree> = Vec::new();
    match &t.kind {
        TreeKind::PackageDef { pid, stats } => {
            v.push(pid);
            v.extend(stats.iter());
        }
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            v.extend(tparams.iter());
            v.extend(vparamss.iter().flatten());
            v.extend(impl_.parents.iter());
            v.extend(impl_.body.iter());
        }
        TreeKind::ModuleDef { impl_, .. } => {
            v.extend(impl_.parents.iter());
            v.extend(impl_.body.iter());
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            v.push(tpt);
            v.push(rhs);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            v.extend(tparams.iter());
            v.extend(vparamss.iter().flatten());
            v.push(tpt);
            v.push(rhs);
        }
        TreeKind::Block { stats, expr } => {
            v.extend(stats.iter());
            v.push(expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            v.push(cond);
            v.push(thenp);
            v.push(elsep);
        }
        TreeKind::Match { selector, cases } => {
            v.push(selector);
            for c in cases {
                v.push(&c.pat);
                v.push(&c.guard);
                v.push(&c.body);
            }
        }
        TreeKind::Function { vparams, body } => {
            v.extend(vparams.iter());
            v.push(body);
        }
        TreeKind::Assign { lhs, rhs } => {
            v.push(lhs);
            v.push(rhs);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
            v.push(cond);
            v.push(body);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => v.push(expr),
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            v.push(block);
            for c in catches {
                v.push(&c.pat);
                v.push(&c.guard);
                v.push(&c.body);
            }
            v.push(finalizer);
        }
        TreeKind::New { tpt } => v.push(tpt),
        TreeKind::Typed { expr, tpt } => {
            v.push(expr);
            v.push(tpt);
        }
        TreeKind::TypeApply { fun, args }
        | TreeKind::Apply { fun, args }
        | TreeKind::UnApply { fun, args } => {
            v.push(fun);
            v.extend(args.iter());
        }
        TreeKind::Select { qual, .. } => v.push(qual),
        TreeKind::Bind { body, .. } => v.push(body),
        TreeKind::Star { elem } => v.push(elem),
        TreeKind::Alternative { trees } => v.extend(trees.iter()),
        TreeKind::LabelDef { params, rhs, .. } => {
            v.extend(params.iter());
            v.push(rhs);
        }
        TreeKind::InterpolatedString { args, .. } => v.extend(args.iter()),
        _ => {}
    }
    v
}

fn collect_captures(
    tree: &Tree,
    own: &HashSet<SymbolId>,
    out: &mut Vec<SymbolId>,
    st: &SymbolTable,
) {
    match &tree.kind {
        TreeKind::Ident { .. } => consider_capture(tree.sym, own, out, st),
        TreeKind::Select { qual, .. } => collect_captures(qual, own, out, st),
        TreeKind::Function { vparams, body } => {
            let mut bound = own.clone();
            for p in vparams {
                if !p.sym.is_none() {
                    bound.insert(p.sym);
                }
            }
            collect_captures(body, &bound, out, st);
        }
        TreeKind::Block { stats, expr } => {
            let mut bound = own.clone();
            for s in stats {
                if let TreeKind::ValDef { .. } = &s.kind {
                    if !s.sym.is_none() {
                        bound.insert(s.sym);
                    }
                }
                collect_captures(s, &bound, out, st);
            }
            collect_captures(expr, &bound, out, st);
        }
        TreeKind::DefDef { vparamss, rhs, .. } => {
            let mut bound = own.clone();
            for p in vparamss.iter().flatten() {
                if !p.sym.is_none() {
                    bound.insert(p.sym);
                }
            }
            collect_captures(rhs, &bound, out, st);
        }
        TreeKind::ValDef { rhs, .. } => collect_captures(rhs, own, out, st),
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            collect_captures(fun, own, out, st);
            for a in args {
                collect_captures(a, own, out, st);
            }
        }
        TreeKind::Typed { expr, .. } => collect_captures(expr, own, out, st),
        TreeKind::If { cond, thenp, elsep } => {
            collect_captures(cond, own, out, st);
            collect_captures(thenp, own, out, st);
            collect_captures(elsep, own, out, st);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            collect_captures(cond, own, out, st);
            collect_captures(body, own, out, st);
        }
        TreeKind::Assign { lhs, rhs } => {
            collect_captures(lhs, own, out, st);
            collect_captures(rhs, own, out, st);
        }
        TreeKind::Match { selector, cases } => {
            collect_captures(selector, own, out, st);
            for c in cases {
                collect_captures(&c.pat, own, out, st);
                collect_captures(&c.guard, own, out, st);
                collect_captures(&c.body, own, out, st);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            collect_captures(block, own, out, st);
            for c in catches {
                collect_captures(&c.pat, own, out, st);
                collect_captures(&c.body, own, out, st);
            }
            collect_captures(finalizer, own, out, st);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            collect_captures(expr, own, out, st)
        }
        TreeKind::LabelDef { rhs, .. } => collect_captures(rhs, own, out, st),
        TreeKind::New { tpt } => {
            collect_captures(tpt, own, out, st);
            // `new F(x)` where `F` is a local class that itself reads an
            // enclosing-method local (`class F(...) { def m = ... factor
            // ... }`) needs that local threaded into *this* nested def too --
            // nothing here references `factor` by name, only `F`'s own
            // constructor call does, once the backend adds the capture as an
            // extra constructor argument. `Symbol::captures` is seeded before
            // this pass runs (see `lambda_lift`) precisely so this is
            // available already.
            if !tpt.sym.is_none() && st.get(tpt.sym).kind == SymKind::Class {
                for c in st.get(tpt.sym).captures.clone() {
                    consider_capture(c, own, out, st);
                }
            }
        }
        TreeKind::ClassDef {
            vparamss, impl_, ..
        } => {
            // `new T { … x … }` inside a nested def reads `x` here, so the
            // lifted method has to receive it too.
            let mut bound = own.clone();
            for p in vparamss.iter().flatten() {
                if !p.sym.is_none() {
                    bound.insert(p.sym);
                }
            }
            for s in &impl_.body {
                if !s.sym.is_none() {
                    bound.insert(s.sym);
                }
            }
            for p in &impl_.parents {
                collect_captures(p, &bound, out, st);
            }
            for s in &impl_.body {
                collect_captures(s, &bound, out, st);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                collect_captures(a, own, out, st);
            }
        }
        _ => {}
    }
}

fn consider_capture(
    id: SymbolId,
    own: &HashSet<SymbolId>,
    out: &mut Vec<SymbolId>,
    st: &SymbolTable,
) {
    if id.is_none() || own.contains(&id) || out.contains(&id) {
        return;
    }
    let s = st.get(id);
    if s.kind != SymKind::Term {
        return;
    }
    let owner_kind = st.get(s.owner).kind;
    if owner_kind != SymKind::Method {
        return;
    }
    out.push(id);
}

fn rewrite_calls(tree: &mut Tree, caps: &HashMap<SymbolId, Vec<SymbolId>>, st: &SymbolTable) {
    match &mut tree.kind {
        TreeKind::PackageDef { stats, .. } => {
            for s in stats {
                rewrite_calls(s, caps, st);
            }
        }
        TreeKind::ClassDef {
            vparamss, impl_, ..
        } => {
            for clause in vparamss {
                for p in clause {
                    rewrite_calls(p, caps, st);
                }
            }
            for p in &mut impl_.parents {
                rewrite_calls(p, caps, st);
            }
            for s in &mut impl_.body {
                rewrite_calls(s, caps, st);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for p in &mut impl_.parents {
                rewrite_calls(p, caps, st);
            }
            for s in &mut impl_.body {
                rewrite_calls(s, caps, st);
            }
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            rewrite_calls(tpt, caps, st);
            rewrite_calls(rhs, caps, st);
        }
        TreeKind::DefDef {
            vparamss, tpt, rhs, ..
        } => {
            for clause in vparamss {
                for p in clause {
                    rewrite_calls(p, caps, st);
                }
            }
            rewrite_calls(tpt, caps, st);
            rewrite_calls(rhs, caps, st);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                rewrite_calls(s, caps, st);
            }
            rewrite_calls(expr, caps, st);
        }
        TreeKind::If { cond, thenp, elsep } => {
            rewrite_calls(cond, caps, st);
            rewrite_calls(thenp, caps, st);
            rewrite_calls(elsep, caps, st);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            rewrite_calls(cond, caps, st);
            rewrite_calls(body, caps, st);
        }
        TreeKind::Function { vparams, body } => {
            for p in vparams {
                rewrite_calls(p, caps, st);
            }
            rewrite_calls(body, caps, st);
        }
        TreeKind::Apply { fun, args } => {
            rewrite_calls(fun, caps, st);
            for a in args.iter_mut() {
                rewrite_calls(a, caps, st);
            }
            let sid = call_sym(fun);
            if let Some(cs) = caps.get(&sid) {
                if !cs.is_empty() {
                    let extra: Vec<Tree> = cs
                        .iter()
                        .map(|&id| capture_ident(id, fun.span, st))
                        .collect();
                    args.splice(0..0, extra);
                }
            }
        }
        TreeKind::TypeApply { fun, args } => {
            rewrite_calls(fun, caps, st);
            for a in args {
                rewrite_calls(a, caps, st);
            }
        }
        TreeKind::Typed { expr, tpt } => {
            rewrite_calls(expr, caps, st);
            rewrite_calls(tpt, caps, st);
        }
        TreeKind::Select { qual, .. } => rewrite_calls(qual, caps, st),
        TreeKind::Ident { .. } => {
            rewrite_auto_apply(tree, caps, st);
        }
        TreeKind::Match { selector, cases } => {
            rewrite_calls(selector, caps, st);
            for c in cases {
                rewrite_calls(&mut c.pat, caps, st);
                rewrite_calls(&mut c.guard, caps, st);
                rewrite_calls(&mut c.body, caps, st);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            rewrite_calls(block, caps, st);
            for c in catches {
                rewrite_calls(&mut c.pat, caps, st);
                rewrite_calls(&mut c.body, caps, st);
            }
            rewrite_calls(finalizer, caps, st);
        }
        TreeKind::Assign { lhs, rhs } => {
            rewrite_calls(lhs, caps, st);
            rewrite_calls(rhs, caps, st);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => rewrite_calls(expr, caps, st),
        TreeKind::New { tpt } => rewrite_calls(tpt, caps, st),
        TreeKind::LabelDef { params, rhs, .. } => {
            for p in params {
                rewrite_calls(p, caps, st);
            }
            rewrite_calls(rhs, caps, st);
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                rewrite_calls(a, caps, st);
            }
        }
        _ => {}
    }
}

fn rewrite_auto_apply(tree: &mut Tree, caps: &HashMap<SymbolId, Vec<SymbolId>>, st: &SymbolTable) {
    if matches!(&tree.ty, Type::Method { .. }) {
        return;
    }
    let sid = tree.sym;
    let Some(cs) = caps.get(&sid) else {
        return;
    };
    if cs.is_empty() {
        return;
    }
    let span = tree.span;
    let mut fun = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    if let Type::Method { .. } = &st.get(sid).ty {
        fun.ty = st.get(sid).ty.clone();
    }
    let args: Vec<Tree> = cs.iter().map(|&id| capture_ident(id, span, st)).collect();
    let result_ty = fun.ty.result().clone();
    *tree = Tree {
        id: NodeId(0),
        span,
        kind: TreeKind::Apply {
            fun: Box::new(fun),
            args,
        },
        ty: result_ty,
        sym: sid,
        postfix: false,
    };
}

fn call_sym(tree: &Tree) -> SymbolId {
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => call_sym(fun),
        _ => tree.sym,
    }
}

fn capture_ident(id: SymbolId, span: scala_rs_span::Span, st: &SymbolTable) -> Tree {
    let s = st.get(id);
    Tree {
        id: NodeId(0),
        span,
        kind: TreeKind::Ident {
            name: s.name.clone(),
        },
        ty: s.ty.clone(),
        sym: id,
        postfix: false,
    }
}
