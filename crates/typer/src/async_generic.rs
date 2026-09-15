//! Bridge the public Scala 2 async transform protocol to continuation lowering.
//! The macro selects the await symbol and supplies its own state-machine class.
use crate::async_lower::GenericAsyncCall;
use crate::check::Typer;
use crate::expand::{at, Sexp};
use crate::lazy_local::children_mut;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    pub(crate) fn generic_async_members(
        &mut self,
        fields: &[Sexp],
        parents: &[Tree],
        members: &[Sexp],
        span: Span,
    ) -> Result<Vec<Tree>, String> {
        if !self.compiler_settings.iter().any(|s| s == "-Xasync") {
            return Err("-Xasync must be enabled for async transformation".into());
        }
        if !self.library_abi {
            return Err("async transformation requires --scala-library".into());
        }
        let original = self.tree_from_reply(at(fields, 3)?, span)?;
        let await_name = at(fields, 4)?.text();
        let propagate = at(fields, 6)?.text() == "1";
        let mut awaitable = self.type_tree_from_wire(at(fields, 5)?, span)?;
        let await_type = self.tree_to_type(&awaitable);
        let await_container = crate::async_lower::await_parameter_key(&self.st, &await_type);
        let TreeKind::DefDef {
            mods,
            name,
            vparamss,
            rhs,
            ..
        } = original.kind
        else {
            return Err("markForAsyncTransform requires a method".into());
        };
        if vparamss.len() != 1 || vparamss[0].len() != 1 {
            return Err("markForAsyncTransform requires one completion parameter".into());
        }
        let TreeKind::ValDef {
            tpt: param_type,
            name: param_name,
            ..
        } = &vparamss[0][0].kind
        else {
            return Err("invalid async completion parameter".into());
        };
        fn parent(t: &Tree) -> &Tree {
            match &t.kind {
                TreeKind::Apply { fun, .. } => parent(fun),
                TreeKind::Select { qual, name } if name == "<init>" => parent(qual),
                TreeKind::New { tpt } => tpt,
                _ => t,
            }
        }
        let mut get_completed = false;
        for p in parents {
            let ty = self.tree_to_type(parent(p));
            self.ensure_pickled_parents(&ty);
            if let Type::Class { sym, .. } = ty {
                self.complete_binary_member(sym, "getCompleted", span);
                get_completed |= !self.st.lookup_member(sym, "getCompleted").is_empty();
                self.complete_binary_member(sym, "onComplete", span);
                if let Some(method) = self.st.lookup_member(sym, "onComplete").first() {
                    if let Type::Method { paramss, .. } = &self.st.get(*method).ty {
                        if let Some(param) = paramss.first().and_then(|ps| ps.first()) {
                            awaitable = Tree::dummy(TreeKind::Ident {
                                name: crate::materialize::RESOLVED_TYPE.into(),
                            });
                            awaitable.ty = param.clone();
                        }
                    }
                }
            }
        }
        for member in members.iter().skip(1) {
            let fields = member.list()?;
            if fields.get(1).map(Sexp::text).as_deref() != Some("DefDef") {
                continue;
            }
            let name = at(at(fields, 4)?.list()?, 2)?.text();
            if name == "getCompleted" {
                get_completed = true;
            }
            if name == "onComplete" {
                let clauses = at(fields, 6)?.list()?;
                if let Some(clause) = clauses.get(1) {
                    if let Some(param) = clause.list()?.get(1) {
                        awaitable = self.tree_from_reply(at(param.list()?, 5)?, span)?;
                    }
                }
            }
        }
        self.any_cto = true;
        self.macro_async_awaits
            .insert((await_name.clone(), await_container.clone()));
        let prefix = self.fresh("async$protocol");
        // Each machine has a small trampoline. Completed awaits and loops run
        // synchronously without recursive Future callbacks growing the stack.
        let mut source = r#"
class AsyncProtocol {
  private var __Ppending: _root_.scala.concurrent.Promise[Any] = null
  private var __Pfailure: Throwable = null
  private class __Pescape(val original: Throwable) extends _root_.scala.util.control.ControlThrowable
  private def __PterminalFailure(t: Throwable): Unit = {
    try completeFailure(t) catch { case cause: Throwable => throw new __Pescape(cause) }
  }
  private val __Pec: _root_.scala.concurrent.ExecutionContext = new _root_.scala.concurrent.ExecutionContext {
    private val queue = new _root_.java.util.ArrayDeque[Runnable]()
    private var running = false
    private def takeNext(): Runnable = queue.synchronized {
      if (queue.isEmpty()) { running = false; null }
      else queue.removeFirst()
    }
    def execute(r: Runnable): Unit = {
      val drain = queue.synchronized {
        queue.addLast(r)
        if (running) false else { running = true; true }
      }
      if (drain) {
        var next = takeNext()
        try { while (next != null) { next.run(); next = takeNext() } }
        finally { if (next != null) queue.synchronized { queue.clear(); running = false } }
      }
    }
    def reportFailure(t: Throwable): Unit = throw t
  }
  private def __Pbridge[T](f: Any): _root_.scala.concurrent.Future[T] = {
    val p = _root_.scala.concurrent.Promise[T]()
    __Ppending = p.asInstanceOf[_root_.scala.concurrent.Promise[Any]]
    state = state + 1
    val completed = getCompleted(f.asInstanceOf[AsyncAwaitable])
    if (completed != null) __Presume(completed)
    else onComplete(f.asInstanceOf[AsyncAwaitable])
    p.future
  }
  private def __Presume(tr: AsyncParam): Unit = {
    val p = __Ppending
    __Ppending = null
    val value = tryGet(tr)
    if (!(value.asInstanceOf[AnyRef] eq this)) { p.success(value); () }
  }
  override def apply(tr: AsyncParam): Unit = {
    try {
    if (state == 0) {
      val result = AsyncBody
      result.onComplete { r =>
        state = -1
        if (r.isSuccess) completeSuccess(r.get.asInstanceOf[AnyRef])
        else __PterminalFailure(if (__Pfailure == null) r.failed.get else __Pfailure)
      }(__Pec)
    } else __Presume(tr)
    } catch { case escaped: __Pescape => throw escaped.original }
  }
}
"#.replace("__P", &prefix);
        if !get_completed {
            let poll = format!("val completed = getCompleted(f.asInstanceOf[AsyncAwaitable])\n    if (completed != null) {prefix}resume(completed)\n    else onComplete(f.asInstanceOf[AsyncAwaitable])");
            source = source.replace(&poll, "onComplete(f.asInstanceOf[AsyncAwaitable])");
        }
        if propagate {
            source = source.replace(
                &format!("try completeFailure(t) catch {{ case cause: Throwable => throw new {prefix}escape(cause) }}"),
                &format!("throw new {prefix}escape(t)"),
            );
            source = source.replace(
                "if (r.isSuccess) completeSuccess(r.get.asInstanceOf[AnyRef])",
                &format!("if (r.isSuccess) try {{ completeSuccess(r.get.asInstanceOf[AnyRef]) }} catch {{ case ex: Throwable => throw new {prefix}escape(ex) }}"),
            );
        } else {
            source = source.replace(
                "if (r.isSuccess) completeSuccess(r.get.asInstanceOf[AnyRef])",
                &format!("if (r.isSuccess) try {{ completeSuccess(r.get.asInstanceOf[AnyRef]) }} catch {{ case ex: Throwable => {prefix}terminalFailure(ex) }}"),
            );
            source = source.replace(
                &format!("else {prefix}resume(tr)"),
                &format!("else try {{ {prefix}resume(tr) }} catch {{ case escaped: {prefix}escape => throw escaped; case ex: Throwable => state = -1; completeFailure(ex) }}"),
            );
        }
        let parsed = scala_rs_parser::parse_str(&source);
        if scala_rs_parser::has_errors(&parsed.diags) {
            return Err(format!(
                "invalid internal async protocol template: {:?}",
                parsed.diags
            ));
        }
        let TreeKind::PackageDef { mut stats, .. } = parsed.tree.kind else {
            unreachable!()
        };
        let TreeKind::ClassDef { mut impl_, .. } = stats.remove(0).kind else {
            unreachable!()
        };
        let call = GenericAsyncCall {
            body: *rhs,
            context: format!("{prefix}ec"),
            bridge: format!("{prefix}bridge"),
            await_name,
            await_container,
            failure: format!("{prefix}failure"),
        };
        fn prepare(
            t: &mut Tree,
            typer: &mut Typer,
            span: Span,
            param: &Tree,
            awaitable: &Tree,
            call: &GenericAsyncCall,
        ) {
            if let TreeKind::Ident { name } = &t.kind {
                match name.as_str() {
                    "AsyncParam" => {
                        *t = param.clone();
                        return;
                    }
                    "AsyncAwaitable" => {
                        *t = awaitable.clone();
                        return;
                    }
                    "AsyncBody" => {
                        t.id = NodeId(typer.macro_next_node);
                        typer.macro_next_node += 1;
                        t.span = span;
                        typer.macro_async_calls.insert(t.id, call.clone());
                        return;
                    }
                    _ => {}
                }
            }
            t.id = NodeId(typer.macro_next_node);
            typer.macro_next_node += 1;
            t.span = span;
            for child in children_mut(t) {
                prepare(child, typer, span, param, awaitable, call);
            }
        }
        for member in &mut impl_.body {
            prepare(member, self, span, param_type, &awaitable, &call);
            if let TreeKind::DefDef {
                mods: m, name: n, ..
            } = &mut member.kind
            {
                if n == "apply" {
                    *n = name.clone();
                    *m = mods.clone();
                    fn rename_param(t: &mut Tree, name: &str) {
                        match &mut t.kind {
                            TreeKind::Ident { name: n } | TreeKind::ValDef { name: n, .. }
                                if n == "tr" =>
                            {
                                *n = name.into()
                            }
                            _ => {}
                        }
                        for child in children_mut(t) {
                            rename_param(child, name);
                        }
                    }
                    rename_param(member, param_name);
                }
            }
        }
        Ok(impl_.body)
    }
}
