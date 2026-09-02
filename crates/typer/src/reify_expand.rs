//! The tree `reify { … }` expands into (`docs/macros.md` §7.14).
//!
//! `reify` is a compiler-internal macro like the quasiquotes: `scala.reflect
//! .api.Universe` declares it, scala-reflect.jar holds no implementation, and
//! nsc short-circuits to a reifier built into the compiler (§6.2). This is
//! scala-rs's, and it builds the shape nsc's `-Xprint:typer` shows:
//!
//! ```text
//! { final class $treecreator1 extends scala.reflect.api.TreeCreator {
//!     def apply[U <: scala.reflect.api.Universe with Singleton](
//!         $m$untyped: scala.reflect.api.Mirror[U]): <Trees.TreeApi> = {
//!       val $u = $m$untyped.universe
//!       val $m = $m$untyped.asInstanceOf[scala.reflect.api.Mirror[$u.type]]
//!       <the body, lowered by crate::reify::Reifier>
//!     }
//!   }
//!   <universe>.Expr.apply[T](
//!     <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $treecreator1()) }
//! ```
//!
//! `tests/fixtures/rd_impl.scala` is this same shape written out by hand and
//! run against real scalac, which is what says the pieces fit: the nested
//! `object Expr` and its hand-written `apply` (`PickleSupply::
//! install_expr_apply`), the `Mirror` cast the pickle's dropped bound makes
//! necessary, and the `WeakTypeTag[T]` the implicit clause draws from
//! `crate::materialize`.
//!
//! **The body is where hygiene lives**, and it is `crate::reify`'s job: a
//! static `object` is rebuilt through `$m.staticModule`, a `.splice` through
//! `Expr.in`, and anything else -- a local, a parameter, `this` -- is refused
//! by name rather than reified as the bare name it was written with.

use scala_rs_parser::{Flags, Modifiers, NodeId, SymbolId, Template, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::materialize::RESOLVED_TYPE;

/// Builds one `reify { … }` expansion; see the module comment for the shape.
pub(crate) struct ReifyExpander<'a> {
    /// The expression naming the universe (`c.universe`), already typed.
    pub(crate) universe: &'a Tree,
    /// Name of the synthetic `TreeCreator` subclass, unique in the run.
    pub(crate) creator_name: String,
    /// The body, already lowered to universe calls.
    pub(crate) body: Tree,
    /// `T` of the resulting `Expr[T]`: the type the body was found to have.
    pub(crate) arg: Type,
    /// `scala.reflect.api.Mirror`, for the cast on the mirror argument.
    pub(crate) mirror_ty: Type,
    /// `scala.reflect.api.Trees.TreeApi`, the creator's erased result.
    pub(crate) tree_api: Type,
    /// The local the creator binds `$m$untyped.universe` to.
    pub(crate) universe_local: String,
    /// The local the creator binds the cast mirror to.
    pub(crate) mirror_local: String,
    /// Whether the body mentions that mirror. A literal-only body does not,
    /// and binding a `val` nothing reads would put a `Mirror[$u.type]` cast
    /// into every `reify { 42 }` for nothing.
    pub(crate) needs_mirror: bool,
    pub(crate) span: Span,
}

impl ReifyExpander<'_> {
    pub(crate) fn build(&self) -> Tree {
        let creator = self.creator_class();
        let call = self.node(TreeKind::Apply {
            fun: Box::new(self.node(TreeKind::TypeApply {
                fun: Box::new(self.select(self.select(self.universe.clone(), "Expr"), "apply")),
                args: vec![self.resolved_type(self.arg.clone())],
            })),
            args: vec![self.mirror(), self.new_creator()],
        });
        self.node(TreeKind::Block {
            stats: vec![creator],
            expr: Box::new(call),
        })
    }

    /// `<universe>.rootMirror.asInstanceOf[scala.reflect.api.Mirror]`.
    ///
    /// The cast is the one `crate::materialize` needs for the same reason:
    /// `rootMirror`'s type is the universe's abstract `Mirror`, whose bound
    /// stops at `JavaMirror` because `api.Mirror[self.type]` is a parent
    /// `conv_upper_bound` drops.
    fn mirror(&self) -> Tree {
        let root = self.select(self.universe.clone(), "rootMirror");
        self.node(TreeKind::TypeApply {
            fun: Box::new(self.select(root, "asInstanceOf")),
            args: vec![self.resolved_type(self.mirror_ty.clone())],
        })
    }

    fn new_creator(&self) -> Tree {
        self.node(TreeKind::Apply {
            fun: Box::new(self.node(TreeKind::New {
                tpt: Box::new(self.node(TreeKind::Ident {
                    name: self.creator_name.clone(),
                })),
            })),
            args: vec![],
        })
    }

    fn creator_class(&self) -> Tree {
        let param = self.node(TreeKind::ValDef {
            mods: Modifiers {
                flags: Flags::PARAM,
                ..Modifiers::default()
            },
            name: UNTYPED_MIRROR.to_string(),
            tpt: Box::new(self.node(TreeKind::AppliedTypeTree {
                tpt: Box::new(self.api_type("Mirror")),
                args: vec![self.node(TreeKind::Ident {
                    name: "U".to_string(),
                })],
            })),
            rhs: Box::new(self.node(TreeKind::Empty)),
        });
        let tparam = self.node(TreeKind::TypeDef {
            mods: Modifiers {
                flags: Flags::PARAM,
                ..Modifiers::default()
            },
            name: "U".to_string(),
            tparams: vec![],
            rhs: Box::new(self.node(TreeKind::Empty)),
            lo: None,
            hi: Some(Box::new(self.node(TreeKind::CompoundTypeTree {
                parents: vec![self.api_type("Universe"), self.scala_type("Singleton")],
                refinements: vec![],
            }))),
            views: vec![],
            ctx_bounds: vec![],
        });
        let apply = self.node(TreeKind::DefDef {
            mods: Modifiers::default(),
            name: "apply".to_string(),
            tparams: vec![tparam],
            vparamss: vec![vec![param]],
            // Written out for the reason `crate::materialize` writes
            // `Types$TypeApi` out: nsc says `U#Tree` and erases it to the
            // bound, scala-rs erases an abstract type member to `Object`, and
            // `TreeCreator.apply` is *abstract* -- a descriptor ending in
            // `Object` overrides nothing and the first call would be an
            // `AbstractMethodError`.
            tpt: Box::new(self.resolved_type(self.tree_api.clone())),
            rhs: Box::new(self.creator_body()),
        });
        self.node(TreeKind::ClassDef {
            mods: Modifiers {
                flags: Flags::FINAL,
                ..Modifiers::default()
            },
            name: self.creator_name.clone(),
            tparams: vec![],
            ctor_mods: Modifiers::default(),
            vparamss: vec![],
            impl_: Template {
                parents: vec![self.api_type("TreeCreator")],
                self_name: None,
                self_tpt: None,
                body: vec![apply],
                span: self.span,
            },
        })
    }

    /// ```text
    /// { val $u = $m$untyped.universe
    ///   val $m = $m$untyped.asInstanceOf[scala.reflect.api.Mirror[$u.type]]
    ///   <body> }
    /// ```
    ///
    /// The universe is bound to a `val` rather than selected afresh at every
    /// use because `$u.type` has to name *one* singleton: `$m`'s cast, and the
    /// `x.in[$u.type]($m)` of every splice, are written against it.
    fn creator_body(&self) -> Tree {
        let mut stats = vec![self.val_def(
            &self.universe_local,
            self.select(self.untyped_mirror(), "universe"),
        )];
        if self.needs_mirror {
            let cast = self.node(TreeKind::TypeApply {
                fun: Box::new(self.select(self.untyped_mirror(), "asInstanceOf")),
                args: vec![self.node(TreeKind::AppliedTypeTree {
                    tpt: Box::new(self.api_type("Mirror")),
                    args: vec![self.node(TreeKind::SingletonTypeTree {
                        ref_: Box::new(self.node(TreeKind::Ident {
                            name: self.universe_local.clone(),
                        })),
                    })],
                })],
            });
            stats.push(self.val_def(&self.mirror_local, cast));
        }
        self.node(TreeKind::Block {
            stats,
            expr: Box::new(self.body.clone()),
        })
    }

    fn val_def(&self, name: &str, rhs: Tree) -> Tree {
        self.node(TreeKind::ValDef {
            mods: Modifiers::default(),
            name: name.to_string(),
            tpt: Box::new(self.node(TreeKind::Empty)),
            rhs: Box::new(rhs),
        })
    }

    fn untyped_mirror(&self) -> Tree {
        self.node(TreeKind::Ident {
            name: UNTYPED_MIRROR.to_string(),
        })
    }

    // -- building blocks ---------------------------------------------------

    fn node(&self, kind: TreeKind) -> Tree {
        Tree {
            id: NodeId(0),
            span: self.span,
            kind,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        }
    }

    fn select(&self, qual: Tree, name: &str) -> Tree {
        self.node(TreeKind::Select {
            qual: Box::new(qual),
            name: name.to_string(),
        })
    }

    /// A type tree that is already a type; `Check::tree_to_type` hands back
    /// its `ty` unchanged.
    fn resolved_type(&self, ty: Type) -> Tree {
        let mut t = self.node(TreeKind::Ident {
            name: RESOLVED_TYPE.to_string(),
        });
        t.ty = ty;
        t
    }

    /// `scala.reflect.api.<name>`, spelled out: the creator's body is typed in
    /// the macro implementation's scope, where `import c.universe._` offers
    /// abstract type members called `Mirror` and `Tree`.
    fn api_type(&self, name: &str) -> Tree {
        let scala = self.node(TreeKind::Ident {
            name: "scala".to_string(),
        });
        let reflect = self.select(scala, "reflect");
        let api = self.select(reflect, "api");
        self.select(api, name)
    }

    fn scala_type(&self, name: &str) -> Tree {
        let scala = self.node(TreeKind::Ident {
            name: "scala".to_string(),
        });
        self.select(scala, name)
    }
}

/// The creator's parameter, named as nsc names it.
const UNTYPED_MIRROR: &str = "$m$untyped";
