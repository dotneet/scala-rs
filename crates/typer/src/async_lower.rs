//! Direct-style scala-async lowering. The library entry points are identified
//! by their resolved symbols, never by an import spelling. Continuations use
//! Future.flatMap so suspension does not occupy an execution-context thread.
//! Original typed subtrees retain their symbols, including locals captured by
//! a continuation. This is deliberately independent of nsc's macro internals.
use crate::check::Typer;
use crate::erasure::for_each_child;
use crate::symbol::{bool_shortcircuit_rhs, SymKind, SymbolTable};
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

type Result = std::result::Result<Tree, (Span, String)>;

fn node(kind: TreeKind, ty: Type, span: Span) -> Tree {
    let mut t = Tree::dummy(kind);
    t.ty = ty;
    t.span = span;
    t
}
fn splice(mut t: Tree) -> Tree {
    t.id = NodeId::PRETYPED_SPLICE;
    t
}
fn unit(span: Span) -> Tree {
    node(TreeKind::Literal { lit: Lit::Unit }, Type::Unit, span)
}
fn block(stats: Vec<Tree>, expr: Tree, span: Span) -> Tree {
    let ty = expr.ty.clone();
    node(
        TreeKind::Block {
            stats,
            expr: Box::new(expr),
        },
        ty,
        span,
    )
}
fn select(qual: Tree, name: &str) -> Tree {
    Tree::dummy(TreeKind::Select {
        qual: Box::new(qual),
        name: name.into(),
    })
}
fn apply(fun: Tree, args: Vec<Tree>) -> Tree {
    Tree::dummy(TreeKind::Apply {
        fun: Box::new(fun),
        args,
    })
}
fn unthunk(t: Tree) -> Tree {
    if t.byname_thunk {
        if let TreeKind::Function { body, .. } = t.kind {
            return *body;
        }
    }
    t
}
fn async_member(st: &SymbolTable, t: &Tree, name: &str) -> bool {
    if !t.sym.is_none() {
        let s = st.get(t.sym);
        if s.name == name && st.jvm_internal(s.owner) == "scala/async/Async$" {
            return true;
        }
    }
    match &t.kind {
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => {
            async_member(st, fun, name)
        }
        _ => false,
    }
}
fn contains_await(st: &SymbolTable, t: &Tree) -> bool {
    if async_member(st, t, "await") {
        return true;
    }
    let mut found = false;
    for_each_child(t, &mut |c| found |= contains_await(st, c));
    found
}

// Declarations directly owned by a continuation, excluding the bodies of
// nested functions/templates. References keep their original symbol identity.
fn continuation_locals(t: &Tree, out: &mut Vec<SymbolId>) {
    if matches!(t.kind, TreeKind::Function { .. }) {
        return;
    }
    match t.kind {
        TreeKind::ValDef { .. } | TreeKind::Bind { .. } => {
            if !t.sym.is_none() {
                out.push(t.sym);
            }
        }
        TreeKind::DefDef { .. }
        | TreeKind::ClassDef { .. }
        | TreeKind::ModuleDef { .. }
        | TreeKind::TypeDef { .. } => {
            if !t.sym.is_none() {
                out.push(t.sym);
            }
            return;
        }
        _ => {}
    }
    for_each_child(t, &mut |c| continuation_locals(c, out));
}

impl Typer {
    pub(crate) fn expand_scala_async(&mut self, tree: &mut Tree, sym: SymbolId) -> bool {
        let s = self.st.get(sym);
        if s.name != "async" || self.st.jvm_internal(s.owner) != "scala/async/Async$" {
            return false;
        }
        let span = tree.span;
        if !self.compiler_settings.iter().any(|s| s == "-Xasync") {
            self.error(span, "The async requires the compiler option -Xasync (supported only by Scala 2.12.12+ / 2.13.3+)");
            tree.ty = Type::Error;
            tree.sym = SymbolId::NONE;
            tree.kind = TreeKind::Empty;
            return true;
        }
        if !self.library_abi {
            self.error(
                span,
                "scala-async requires --scala-library and the scala-async jar",
            );
            tree.ty = Type::Error;
            tree.kind = TreeKind::Empty;
            tree.sym = SymbolId::NONE;
            return true;
        }
        let mut clauses = Vec::new();
        let mut t = &*tree;
        loop {
            match &t.kind {
                TreeKind::Apply { fun, args } => {
                    clauses.push(args.clone());
                    t = fun;
                }
                TreeKind::TypeApply { fun, .. } => t = fun,
                _ => break,
            }
        }
        clauses.reverse();
        if clauses.len() == 1 && clauses[0].len() == 2 {
            let ec = clauses[0].pop().unwrap();
            clauses.push(vec![ec]);
        }
        if clauses.len() != 2 || clauses.iter().any(|c| c.len() != 1) {
            return false;
        }
        let body = unthunk(clauses[0].remove(0));
        let context = clauses[1].remove(0);
        let mut lower = Lower {
            typer: self,
            context: context.clone(),
        };
        // Capture the implicit expression once, on the caller, just as the
        // FutureStateMachine constructor does. All callbacks share that value.
        let (ec_def, ec_ref) = lower.local("async$ec", context.ty.clone(), context, false);
        lower.context = ec_ref;
        let lowered = lower.validate(&body).and_then(|()| {
            let body = lower.lower(body)?;
            let start = lower.success(unit(span));
            let (param, _) = lower.local("async$start", Type::Unit, unit(span), true);
            Ok(lower.flat_map(start, param, body))
        });
        match lowered {
            Ok(mut result) => {
                // Preserve the public macro result (not a singleton inferred
                // inside a particular branch of the continuation).
                result.ty = tree.ty.clone();
                *tree = splice(block(vec![ec_def], result, span));
            }
            Err((at, why)) => {
                lower.typer.error(at, why);
                *tree = node(TreeKind::Empty, Type::Error, span);
            }
        }
        true
    }
}

struct Lower<'a> {
    typer: &'a mut Typer,
    context: Tree,
}
impl Lower<'_> {
    fn has(&self, t: &Tree) -> bool {
        contains_await(&self.typer.st, t)
    }
    fn validate(&self, t: &Tree) -> std::result::Result<(), (Span, String)> {
        // Boolean &&/|| have by-name parameters, but are compiler control flow.
        if bool_shortcircuit_rhs(&self.typer.st, t).is_some() {
            if let TreeKind::Apply { fun, args } = &t.kind {
                self.validate(fun)?;
                return self.validate(&unthunk(args[0].clone()));
            }
        }
        let forbidden = match &t.kind {
            TreeKind::Function { .. } if t.byname_thunk => Some("a by-name argument"),
            TreeKind::Function { .. } => Some("a nested function"),
            TreeKind::DefDef { .. } => Some("a nested method"),
            TreeKind::ClassDef { .. } => Some("a nested class"),
            TreeKind::ModuleDef { .. } => Some("a nested object"),
            TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::LAZY) => Some("a lazy val"),
            TreeKind::Try { .. } => Some("try/catch/finally"),
            _ => None,
        };
        if let Some(place) = forbidden {
            if self.has(t) {
                return Err((t.span, format!("await must not be used under {place}")));
            }
            return Ok(());
        }
        if matches!(t.kind, TreeKind::Return { .. }) {
            return Err((t.span, "return is not allowed in an async block".into()));
        }
        let mut result = Ok(());
        for_each_child(t, &mut |c| {
            if result.is_ok() {
                result = self.validate(c);
            }
        });
        result
    }
    fn local(&mut self, prefix: &str, ty: Type, rhs: Tree, param: bool) -> (Tree, Tree) {
        let name = self.typer.fresh(prefix);
        let flags = if param {
            Flags::SYNTHETIC.with(Flags::PARAM)
        } else {
            Flags::SYNTHETIC
        };
        let sym = self
            .typer
            .st
            .alloc(&name, self.typer.st.owner, SymKind::Term, flags, "");
        self.typer.st.get_mut(sym).ty = ty.clone();
        let span = rhs.span;
        let mut def = node(
            TreeKind::ValDef {
                mods: Modifiers {
                    flags,
                    ..Default::default()
                },
                name: name.clone(),
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(if param {
                    Tree::dummy(TreeKind::Empty)
                } else {
                    rhs
                }),
            },
            ty.clone(),
            span,
        );
        def.sym = sym;
        let mut reference = node(TreeKind::Ident { name }, ty, span);
        reference.sym = sym;
        (def, splice(reference))
    }
    fn success(&mut self, expr: Tree) -> Tree {
        let mut path = Tree::dummy(TreeKind::Ident {
            name: "_root_".into(),
        });
        for name in ["scala", "concurrent", "Future", "successful"] {
            path = select(path, name);
        }
        let span = expr.span;
        let mut result = apply(path, vec![splice(block(vec![], expr, span))]);
        self.typer.type_expr(&mut result, &Type::NoType);
        splice(result)
    }
    fn flat_map(&mut self, future: Tree, param: Tree, body: Tree) -> Tree {
        let param_sym = param.sym;
        let mut locals = Vec::new();
        continuation_locals(&body, &mut locals);
        // A fresh wrapper gives type_function a distinct node/owner for each
        // continuation; the reserved PRETYPED_SPLICE id is shared by splices.
        let lambda = Tree::dummy(TreeKind::Function {
            vparams: vec![param],
            body: Box::new(Tree::dummy(TreeKind::Block {
                stats: vec![],
                expr: Box::new(splice(body)),
            })),
        });
        let mut result = apply(
            apply(select(splice(future), "flatMap"), vec![lambda]),
            vec![splice(self.context.clone())],
        );
        self.typer.type_expr(&mut result, &Type::NoType);
        let owner = self.typer.st.get(param_sym).owner;
        // The body was typed in its original scope. In a field initializer,
        // its locals can still have the enclosing class as owner. The backend
        // then captures a mutable local by value instead of sharing a Ref cell.
        for sym in locals {
            let previous = self.typer.st.get(sym).owner;
            if previous != owner {
                if !previous.is_none() {
                    self.typer
                        .st
                        .get_mut(previous)
                        .members
                        .retain(|s| *s != sym);
                }
                self.typer.st.get_mut(sym).owner = owner;
                self.typer.macro_mirror_owners.insert(sym, owner);
                if !self.typer.st.get(owner).members.contains(&sym) {
                    self.typer.st.get_mut(owner).members.push(sym);
                }
            }
        }
        splice(result)
    }
    fn lower(&mut self, t: Tree) -> Result {
        let span = t.span;
        let ty = t.ty.clone();
        if !self.has(&t) {
            return Ok(self.success(t));
        }
        if async_member(&self.typer.st, &t, "await") {
            if let TreeKind::Apply { mut args, .. } = t.kind {
                let arg = args.remove(0);
                if !self.has(&arg) {
                    // Map through the continuation even for a terminal await:
                    // a null Future must fail the async result, not escape as null.
                    let (param, value) = self.local("async$await", ty, unit(span), true);
                    let result = self.success(value);
                    return Ok(self.flat_map(arg, param, result));
                }
                let fty = arg.ty.clone();
                let future = self.lower(arg)?;
                let (param, value) = self.local("async$future", fty, unit(span), true);
                let (inner, result) = self.local("async$await", ty, unit(span), true);
                let result = self.success(result);
                let body = self.flat_map(value, inner, result);
                return Ok(self.flat_map(future, param, body));
            }
        }
        if bool_shortcircuit_rhs(&self.typer.st, &t).is_some() {
            if let TreeKind::Apply { fun, args } = t.kind {
                if let TreeKind::Select { qual, name } = fun.kind {
                    let rhs = unthunk(args[0].clone());
                    let lit = node(
                        TreeKind::Literal {
                            lit: Lit::Boolean(name == "||" || name == "$bar$bar"),
                        },
                        Type::Boolean,
                        span,
                    );
                    let (thenp, elsep) = if name == "&&" || name == "$amp$amp" {
                        (rhs, lit)
                    } else {
                        (lit, rhs)
                    };
                    return self.lower(node(
                        TreeKind::If {
                            cond: qual,
                            thenp: Box::new(thenp),
                            elsep: Box::new(elsep),
                        },
                        ty,
                        span,
                    ));
                }
            }
            unreachable!();
        }
        match t.kind {
            TreeKind::Block { stats, expr } => {
                let mut tail = self.lower(*expr)?;
                for mut stat in stats.into_iter().rev() {
                    if !self.has(&stat) {
                        tail = block(vec![stat], tail, span);
                        continue;
                    }
                    if let TreeKind::ValDef { rhs, .. } = &mut stat.kind {
                        let init = self.lower((**rhs).clone())?;
                        let (param, value) =
                            self.local("async$value", stat.ty.clone(), unit(span), true);
                        **rhs = value;
                        tail = self.flat_map(init, param, block(vec![stat], tail, span));
                    } else {
                        let init = self.lower(stat.clone())?;
                        let (param, _) =
                            self.local("async$discard", stat.ty.clone(), unit(span), true);
                        tail = self.flat_map(init, param, tail);
                    }
                }
                Ok(tail)
            }
            TreeKind::If { cond, thenp, elsep } => {
                let thenp = self.lower(*thenp)?;
                let elsep = self.lower(*elsep)?;
                let result_ty = self.future_type(&thenp.ty, ty.clone());
                let (def, value) = self.local("async$condition", cond.ty.clone(), unit(span), true);
                let body = node(
                    TreeKind::If {
                        cond: Box::new(value),
                        thenp: Box::new(thenp),
                        elsep: Box::new(elsep),
                    },
                    result_ty,
                    span,
                );
                if self.has(&cond) {
                    let f = self.lower(*cond)?;
                    Ok(self.flat_map(f, def, body))
                } else {
                    let mut body = body;
                    if let TreeKind::If { cond: slot, .. } = &mut body.kind {
                        *slot = cond;
                    }
                    Ok(body)
                }
            }
            TreeKind::Match { selector, cases } => {
                let suspends = self.has(&selector);
                let (param, value) =
                    self.local("async$selector", selector.ty.clone(), unit(span), suspends);
                let body = self.lower_cases(value, cases, ty, span)?;
                if suspends {
                    let f = self.lower(*selector)?;
                    Ok(self.flat_map(f, param, body))
                } else {
                    let mut def = param;
                    if let TreeKind::ValDef { rhs, .. } = &mut def.kind {
                        *rhs = selector;
                    }
                    Ok(block(vec![def], body, span))
                }
            }
            TreeKind::While { cond, body } => self.lower_loop(*cond, *body, false, span),
            TreeKind::DoWhile { body, cond } => self.lower_loop(*cond, *body, true, span),
            kind => {
                let mut rebuilt = node(kind, ty, span);
                rebuilt.sym = t.sym;
                let mut steps = Vec::new();
                self.operands(&mut rebuilt, &mut steps)?;
                let mut tail = self.success(rebuilt);
                for (mut def, expr) in steps.into_iter().rev() {
                    if self.has(&expr) {
                        let f = self.lower(expr)?;
                        if let TreeKind::ValDef { mods, .. } = &mut def.kind {
                            mods.flags = mods.flags.with(Flags::PARAM);
                        }
                        self.typer.st.get_mut(def.sym).flags =
                            self.typer.st.get(def.sym).flags.with(Flags::PARAM);
                        tail = self.flat_map(f, def, tail);
                    } else {
                        if let TreeKind::ValDef { rhs, .. } = &mut def.kind {
                            **rhs = expr;
                        }
                        tail = block(vec![def], tail, span);
                    }
                }
                Ok(tail)
            }
        }
    }
    fn operand(&mut self, t: &mut Tree, steps: &mut Vec<(Tree, Tree)>) {
        // By-name arguments without await retain their thunk and laziness.
        if t.byname_thunk {
            return;
        }
        let (def, reference) = self.local("async$operand", t.ty.clone(), unit(t.span), false);
        let old = std::mem::replace(t, reference);
        steps.push((def, old));
    }
    fn callee(
        &mut self,
        t: &mut Tree,
        steps: &mut Vec<(Tree, Tree)>,
    ) -> std::result::Result<(), (Span, String)> {
        match &mut t.kind {
            TreeKind::Select { qual, .. } => {
                if !matches!(qual.kind, TreeKind::New { .. } | TreeKind::Super { .. }) {
                    self.operand(qual, steps);
                }
            }
            TreeKind::TypeApply { fun, .. } => self.callee(fun, steps)?,
            TreeKind::Apply { fun, args } => {
                self.callee(fun, steps)?;
                for arg in args {
                    self.operand(arg, steps);
                }
            }
            TreeKind::Ident { .. } | TreeKind::New { .. } => {}
            _ => self.operand(t, steps),
        }
        Ok(())
    }
    fn operands(
        &mut self,
        t: &mut Tree,
        steps: &mut Vec<(Tree, Tree)>,
    ) -> std::result::Result<(), (Span, String)> {
        match &mut t.kind {
            TreeKind::TypeApply { fun, .. } => self.callee(fun, steps)?,
            TreeKind::Apply { fun, args } => {
                self.callee(fun, steps)?;
                for arg in args {
                    self.operand(arg, steps);
                }
            }
            TreeKind::Select { qual, .. } => self.operand(qual, steps),
            TreeKind::Typed { expr, .. } | TreeKind::Throw { expr } => self.operand(expr, steps),
            TreeKind::Assign { lhs, rhs } => {
                if let TreeKind::Select { qual, .. } = &mut lhs.kind {
                    self.operand(qual, steps);
                }
                self.operand(rhs, steps);
            }
            _ => {
                return Err((
                    t.span,
                    "async lowering does not support await in this expression".into(),
                ))
            }
        }
        Ok(())
    }
    fn future_type(&self, sample: &Type, elem: Type) -> Type {
        match sample {
            Type::Class { sym, .. } => Type::Class {
                sym: *sym,
                args: vec![elem.widen_constant()],
            },
            _ => sample.clone(),
        }
    }
    fn method(&mut self, prefix: &str, result_ty: Type, span: Span) -> (Tree, Tree) {
        let name = self.typer.fresh(prefix);
        let sym = self.typer.st.alloc(
            &name,
            self.typer.st.owner,
            SymKind::Method,
            Flags::SYNTHETIC,
            "",
        );
        let method_ty = Type::Method {
            paramss: vec![vec![]],
            ret: Box::new(result_ty.clone()),
        };
        self.typer.st.get_mut(sym).ty = method_ty.clone();
        self.typer.st.get_mut(sym).paramss = vec![vec![]];
        let mut reference = node(
            TreeKind::Ident { name: name.clone() },
            method_ty.clone(),
            span,
        );
        reference.sym = sym;
        let mut call = apply(reference, vec![]);
        call.sym = sym;
        call.ty = result_ty;
        let mut method = node(
            TreeKind::DefDef {
                mods: Modifiers {
                    flags: Flags::SYNTHETIC,
                    ..Default::default()
                },
                name,
                tparams: vec![],
                vparamss: vec![vec![]],
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            method_ty,
            span,
        );
        method.sym = sym;
        (method, call)
    }
    fn lower_cases(
        &mut self,
        selector: Tree,
        mut cases: Vec<CaseDef>,
        ty: Type,
        span: Span,
    ) -> Result {
        let Some(index) = cases.iter().position(|c| self.has(&c.guard)) else {
            for c in &mut cases {
                c.body = self.lower(c.body.clone())?;
            }
            let sample = self.success(unit(span));
            return Ok(node(
                TreeKind::Match {
                    selector: Box::new(selector),
                    cases,
                },
                self.future_type(&sample.ty, ty),
                span,
            ));
        };
        let rest = cases.split_off(index + 1);
        let fallback = self.lower_cases(selector.clone(), rest, ty.clone(), span)?;
        let (mut method, call) = self.method("async$case", fallback.ty.clone(), span);
        if let TreeKind::DefDef { rhs, .. } = &mut method.kind {
            **rhs = fallback;
        }
        for c in &mut cases[..index] {
            c.body = self.lower(c.body.clone())?;
        }
        let guarded = &mut cases[index];
        let guard = std::mem::replace(&mut guarded.guard, Tree::dummy(TreeKind::Empty));
        let yes = self.lower(guarded.body.clone())?;
        let (param, value) = self.local("async$guard", Type::Boolean, unit(span), true);
        let branch = node(
            TreeKind::If {
                cond: Box::new(value),
                thenp: Box::new(yes),
                elsep: Box::new(call.clone()),
            },
            call.ty.clone(),
            span,
        );
        let guard = self.lower(guard)?;
        guarded.body = self.flat_map(guard, param, branch);
        let pat = &cases[index].pat;
        if !crate::warn_patmat_translate::Translator::is_wildcard(pat)
            && !crate::warn_patmat_translate::Translator::is_var_pattern(pat)
        {
            cases.push(CaseDef {
                pat: Tree::dummy(TreeKind::Wildcard),
                guard: Tree::dummy(TreeKind::Empty),
                body: call.clone(),
                span,
            });
        }
        let matched = node(
            TreeKind::Match {
                selector: Box::new(selector),
                cases,
            },
            call.ty,
            span,
        );
        Ok(block(vec![method], matched, span))
    }
    fn lower_loop(&mut self, cond: Tree, body: Tree, first_body: bool, span: Span) -> Result {
        let done = self.success(unit(span));
        let (mut method, recur) = self.method("async$loop", done.ty.clone(), span);
        let body_ty = body.ty.clone();
        let body = self.lower(body)?;
        let (param, _) = self.local("async$iteration", body_ty, unit(span), true);
        let next = self.flat_map(body, param, recur.clone());
        let (param, value) = self.local("async$condition", Type::Boolean, unit(span), true);
        let branch = node(
            TreeKind::If {
                cond: Box::new(value),
                thenp: Box::new(next.clone()),
                elsep: Box::new(done.clone()),
            },
            done.ty.clone(),
            span,
        );
        let test = self.lower(cond)?;
        let test = self.flat_map(test, param, branch);
        if let TreeKind::DefDef { rhs, .. } = &mut method.kind {
            **rhs = test;
        }
        Ok(block(
            vec![method],
            if first_body { next } else { recur },
            span,
        ))
    }
}
