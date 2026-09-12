//! The tree `reify { … }` expands into (`docs/macros.md` §7.14,
//! `docs/notes/reify-design.md`).
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
//!       <the body, lowered by crate::reify::Reifier -- free symbols first>
//!     }
//!   }
//!   <universe>.Expr.apply[T](
//!     <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $treecreator1()) }
//! ```
//!
//! The `WeakTypeTag[T]` that `Expr.apply` demands is left to the implicit
//! materialiser (`crate::materialize`) -- unless `T` mentions a free type,
//! which only a `TypeCreator` sharing the body's symbol table can build. That
//! creator is then written out here, as nsc always writes it:
//!
//! ```text
//!   <universe>.Expr.apply[T](<mirror>, new $treecreator1())(
//!     <universe>.WeakTypeTag.apply[T](<mirror>, {
//!       final class $typecreator1 extends scala.reflect.api.TypeCreator {
//!         def apply[U <: ...]($m$untyped: Mirror[U]): <Types.TypeApi> = {
//!           val $u = ...; val $m = ...; <free types>; <the type> } }
//!       new $typecreator1() }))
//! ```
//!
//! `tests/fixtures/rd_impl.scala` is the first shape written out by hand and
//! run against real scalac, which is what says the pieces fit.

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
    /// `scala.reflect.api.Trees.TreeApi`, the tree creator's erased result.
    pub(crate) tree_api: Type,
    /// `scala.reflect.api.Types.TypeApi`, the type creator's erased result.
    pub(crate) type_api: Type,
    /// The local the creator binds `$m$untyped.universe` to.
    pub(crate) universe_local: String,
    /// The local the creator binds the cast mirror to.
    pub(crate) mirror_local: String,
    /// The type creator's body, when the tag has to be built here: the
    /// type value of `arg`, with its free types bound in front.
    pub(crate) tag: Option<Tree>,
    /// Name of the synthetic `TypeCreator` subclass, if one is written.
    pub(crate) tag_creator_name: String,
    /// The tags in scope the body splices, each bound to a local ahead of
    /// the creator (`val $tag1 = evidence$1`): a creator is a local class,
    /// and a class parameter's field is private to the class that declares
    /// it, so the creator reads the local instead of the field.
    pub(crate) tag_bindings: Vec<(String, Tree)>,
    pub(crate) span: Span,
}

impl ReifyExpander<'_> {
    pub(crate) fn build(&self) -> Tree {
        let creator = self.creator_class(
            &self.creator_name,
            "TreeCreator",
            self.tree_api.clone(),
            &self.body,
        );
        let mut call = self.node(TreeKind::Apply {
            fun: Box::new(self.node(TreeKind::TypeApply {
                fun: Box::new(self.select(self.select(self.universe.clone(), "Expr"), "apply")),
                args: vec![self.resolved_type(self.arg.clone())],
            })),
            args: vec![self.mirror(), self.new_creator(&self.creator_name)],
        });
        if let Some(tag_body) = &self.tag {
            let tag_creator = self.creator_class(
                &self.tag_creator_name,
                "TypeCreator",
                self.type_api.clone(),
                tag_body,
            );
            let tag = self.node(TreeKind::Apply {
                fun: Box::new(self.node(TreeKind::TypeApply {
                    fun: Box::new(
                        self.select(self.select(self.universe.clone(), "WeakTypeTag"), "apply"),
                    ),
                    args: vec![self.resolved_type(self.arg.clone())],
                })),
                args: vec![
                    self.mirror(),
                    self.node(TreeKind::Block {
                        stats: vec![tag_creator],
                        expr: Box::new(self.new_creator(&self.tag_creator_name)),
                    }),
                ],
            });
            call = self.node(TreeKind::Apply {
                fun: Box::new(call),
                args: vec![tag],
            });
        }
        let mut stats: Vec<Tree> = self
            .tag_bindings
            .iter()
            .map(|(name, tree)| self.val_def(name, tree.clone()))
            .collect();
        stats.push(creator);
        self.node(TreeKind::Block {
            stats,
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

    fn new_creator(&self, name: &str) -> Tree {
        self.node(TreeKind::Apply {
            fun: Box::new(self.node(TreeKind::New {
                tpt: Box::new(self.node(TreeKind::Ident {
                    name: name.to_string(),
                })),
            })),
            args: vec![],
        })
    }

    /// `final class <name> extends scala.reflect.api.<parent> { def apply[U
    /// <: Universe with Singleton]($m$untyped: Mirror[U]): <result> = { val
    /// $u = ...; val $m = ...; <body> } }`.
    fn creator_class(&self, name: &str, parent: &str, result: Type, body: &Tree) -> Tree {
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
            tpt: Box::new(self.resolved_type(result)),
            rhs: Box::new(self.creator_body(body)),
        });
        self.node(TreeKind::ClassDef {
            mods: Modifiers {
                flags: Flags::FINAL,
                ..Modifiers::default()
            },
            name: name.to_string(),
            tparams: vec![],
            ctor_mods: Modifiers::default(),
            vparamss: vec![],
            impl_: Template {
                parents: vec![self.api_type(parent)],
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
    /// `x.in[$u.type]($m)` of every splice, are written against it. The
    /// mirror is bound only when the body mentions it: a literal-only body
    /// does not, and a `Mirror[$u.type]` cast nothing reads would sit in
    /// every `reify { 42 }` for nothing.
    fn creator_body(&self, body: &Tree) -> Tree {
        let mut stats = vec![self.val_def(
            &self.universe_local,
            self.select(self.untyped_mirror(), "universe"),
        )];
        if mentions(body, &self.mirror_local) {
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
            expr: Box::new(body.clone()),
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
            scala_ref: false,
            stable_pat: false,
            byname_thunk: false,
            byname_type_marker: false,
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

/// Whether `t` mentions the identifier `name` anywhere.
fn mentions(t: &Tree, name: &str) -> bool {
    match &t.kind {
        TreeKind::Ident { name: n } => n == name,
        TreeKind::Select { qual, .. } => mentions(qual, name),
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            mentions(fun, name) || args.iter().any(|a| mentions(a, name))
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().any(|s| mentions(s, name)) || mentions(expr, name)
        }
        TreeKind::ValDef { tpt, rhs, .. } => mentions(tpt, name) || mentions(rhs, name),
        TreeKind::Function { vparams, body } => {
            vparams.iter().any(|p| mentions(p, name)) || mentions(body, name)
        }
        TreeKind::Typed { expr, tpt } => mentions(expr, name) || mentions(tpt, name),
        TreeKind::New { tpt } => mentions(tpt, name),
        TreeKind::AppliedTypeTree { tpt, args } => {
            mentions(tpt, name) || args.iter().any(|a| mentions(a, name))
        }
        TreeKind::SingletonTypeTree { ref_ } => mentions(ref_, name),
        TreeKind::If { cond, thenp, elsep } => {
            mentions(cond, name) || mentions(thenp, name) || mentions(elsep, name)
        }
        TreeKind::Match { selector, cases } => {
            mentions(selector, name)
                || cases.iter().any(|c| {
                    mentions(&c.pat, name) || mentions(&c.guard, name) || mentions(&c.body, name)
                })
        }
        _ => false,
    }
}

/// The creator's parameter, named as nsc names it.
const UNTYPED_MIRROR: &str = "$m$untyped";
