//! `scala.reflect.runtime.currentMirror`, expanded by the compiler itself.
//!
//! nsc has a small table of macros it expands *without* running an
//! implementation: `scala.tools.reflect.FastTrack` maps a macro symbol's full
//! name straight to a function of the compiler's own. `currentMirror` is one
//! of them. Its declaration in `scala/reflect/runtime/package.scala` is
//!
//! ```text
//! // implementation hardwired to the `currentMirror` method below
//! // using the mechanism implemented in `scala.tools.reflect.FastTrack`
//! def currentMirror: universe.Mirror = macro ???
//! ```
//!
//! -- the `@macroImpl` annotation on the real classfile is the placeholder
//! `???`, so there is nothing for the ordinary "read the annotation, load the
//! class, invoke the method" path (`crates/typer/src/expand.rs`) to call.
//! [`PickleSupply::install_known_macro`] already supplies the *binding* by
//! name, the same way `FastTrack` does; this module supplies the *expansion*.
//!
//! Running `scala/reflect/runtime/Macros$.currentMirror` through the JVM
//! bridge instead would need two more `Context` members than the bridge has
//! (`c.reifyEnclosingRuntimeClass`, whose result is a `Literal(Constant(<a
//! type>))` the reply protocol cannot carry) and would arrive at exactly the
//! tree below. Measured against real scalac 2.13.16 with
//! `-Ymacro-debug-lite`, the expansion of `currentMirror` inside `object Test`
//! is
//!
//! ```text
//! _root_.scala.reflect.runtime.universe.runtimeMirror(this.getClass.getClassLoader)
//! ```
//!
//! (`Apply(Select(… TermName("runtimeMirror")), List(Select(Select(This(
//! typeNames.EMPTY), TermName("getClass")), TermName("getClassLoader"))))`),
//! which is what [`Typer::expand_current_mirror`] builds. The implementation's
//! own `if (runtimeClass.isEmpty) c.abort(…, "call site does not have an
//! enclosing class")` is kept as the `Err` below: a call site with no
//! enclosing class gets a diagnostic, not a guess.

use scala_rs_parser::{NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::MacroBinding;

/// The implementation `install_known_macro` binds `currentMirror` to.
const CURRENT_MIRROR: (&str, &str) = ("scala/reflect/runtime/Macros$", "currentMirror");

impl Typer {
    /// The expansion of a macro this compiler implements itself, if this is
    /// one of them. `None` means "not fast-tracked", and the caller goes on to
    /// the JVM bridge exactly as before.
    pub(crate) fn fasttrack_expansion(
        &mut self,
        binding: &MacroBinding,
        span: Span,
    ) -> Option<Result<Tree, String>> {
        if (binding.impl_class.as_str(), binding.impl_method.as_str()) == CURRENT_MIRROR {
            return Some(self.expand_current_mirror(span));
        }
        None
    }

    fn expand_current_mirror(&mut self, span: Span) -> Result<Tree, String> {
        // nsc's own guard, kept: `reifyEnclosingRuntimeClass` returns
        // `EmptyTree` where there is no enclosing class to take a class loader
        // from, and the implementation aborts rather than picking one.
        if self.st.this_class.is_none() {
            return Err("the call site has no enclosing class to take a \
                        class loader from"
                .to_string());
        }
        let node = |kind: TreeKind| Tree {
            id: NodeId(0),
            span,
            kind,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let select = |qual: Tree, name: &str| {
            node(TreeKind::Select {
                qual: Box::new(qual),
                name: name.to_string(),
            })
        };
        // `_root_.scala.reflect.runtime.universe`, spelled from the root the
        // way nsc's expansion does: the call site may well have its own
        // `scala` or `runtime` in scope.
        let root = node(TreeKind::Ident {
            name: "_root_".to_string(),
        });
        let universe = select(
            select(select(select(root, "scala"), "reflect"), "runtime"),
            "universe",
        );
        let loader = select(
            select(node(TreeKind::This { qual: None }), "getClass"),
            "getClassLoader",
        );
        Ok(node(TreeKind::Apply {
            fun: Box::new(select(universe, "runtimeMirror")),
            args: vec![loader],
        }))
    }
}
