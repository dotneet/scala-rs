//! `TypeTag[T]` / `WeakTypeTag[T]` materialisation (`docs/macros.md` §7.10).
//!
//! `def typeOf[T](implicit ttag: TypeTag[T]): Type` is written so that the
//! *tag* carries the type: nothing in a program ever writes a `TypeTag`
//! literal, so when the implicit search comes up empty nsc does not report a
//! missing implicit -- it expands the compiler-internal macro
//! `materializeTypeTag[T](u)`, which synthesises a `TypeCreator` that rebuilds
//! `T` inside whatever universe the tag is later handed to. Without that,
//! `c.typeOf[HList]` (slick's `ShapedValue.mapToImpl`) and every
//! `implicitly[TypeTag[T]]` are dead ends.
//!
//! The shape is nsc's, read off `-Ymacro-debug-lite` / `-Xprint:typer`:
//!
//! ```text
//! {
//!   final class $typecreator1 extends scala.reflect.api.TypeCreator {
//!     def apply[U <: scala.reflect.api.Universe with Singleton](
//!         $m$untyped: scala.reflect.api.Mirror[U]): <Types.TypeApi> =
//!       $m$untyped.staticClass("Foo").asType.toTypeConstructor
//!   }
//!   <universe>.TypeTag.apply[Foo](
//!     <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $typecreator1())
//! }
//! ```
//!
//! **Three deliberate differences from nsc**, all recorded in
//! `docs/macros.md` §7.10 and all covered by the dual run in
//! `crates/cli/tests/quasi.rs`:
//!
//! 1. nsc binds `$u` / `$m` to `val`s and writes the creator's body against
//!    them; the body here selects on the mirror parameter directly. The tree
//!    is smaller, and `tag.tpe` is the same type -- which is what the fixture
//!    compares, `=:=` and `toString`, against real scalac.
//! 2. nsc reaches the runtime universe's mirror with
//!    `runtimeMirror(getClass.getClassLoader)` and a macro context's with
//!    `rootMirror`; scala-rs uses `rootMirror` for both, because
//!    `JavaUniverse#runtimeMirror` is not a member scala-rs can supply yet
//!    (its `java.lang.ClassLoader` parameter has no symbol). The two differ
//!    only for a class the root mirror's class loader cannot see, and that
//!    case raises `ScalaReflectionException` rather than going quiet.
//! 3. nsc writes the creator's result as `U#Type` and its own erasure turns
//!    that into `Types$TypeApi`; scala-rs erases an abstract type member to
//!    `Object` (`erasure::erase_ty`), which would leave `TypeCreator.apply`
//!    unimplemented and `tag.tpe` throwing `AbstractMethodError`. The bound
//!    is therefore written out. Likewise the mirror argument is cast:
//!    `rootMirror`'s type is the universe's abstract `Mirror`, whose pickled
//!    bound stops at `JavaMirror` because `api.Mirror[self.type]` is a parent
//!    `conv_upper_bound` drops.
//!
//! **Only a monomorphic type is built.** `staticClass(<name>)` names one
//! class; a type constructor applied to arguments, an abstract type, a
//! singleton or a structural type each need a different piece of nsc's
//! reifier, and each is *named* in a diagnostic rather than approximated --
//! building the wrong `Type` would be discovered as a wrong answer at run
//! time, long after the compile.

use scala_rs_parser::{Flags, Lit, Modifiers, NodeId, SymbolId, Template, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::symbol::{SymKind, SymbolTable};

/// `scala.reflect.api.TypeTags#TypeTag`, as the class file names it.
const TYPE_TAG: &str = "scala/reflect/api/TypeTags$TypeTag";
/// `scala.reflect.api.TypeTags#WeakTypeTag`.
const WEAK_TYPE_TAG: &str = "scala/reflect/api/TypeTags$WeakTypeTag";
const MIRROR: &str = "scala/reflect/api/Mirror";
const TYPE_CREATOR: &str = "scala/reflect/api/TypeCreator";

/// Which of the two tags an implicit request asks for.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tag {
    Strong,
    Weak,
}

impl Tag {
    pub(crate) fn jvm(self) -> &'static str {
        match self {
            Tag::Strong => TYPE_TAG,
            Tag::Weak => WEAK_TYPE_TAG,
        }
    }

    /// How a pickle spells the class: `Outer.Inner`, not `Outer$Inner`.
    pub(crate) fn pickle_name(self) -> &'static str {
        match self {
            Tag::Strong => "scala.reflect.api.TypeTags.TypeTag",
            Tag::Weak => "scala.reflect.api.TypeTags.WeakTypeTag",
        }
    }

    /// The name the universe's accessor and the companion object share.
    pub(crate) fn simple(self) -> &'static str {
        match self {
            Tag::Strong => "TypeTag",
            Tag::Weak => "WeakTypeTag",
        }
    }
}

/// Whether `pt` is `TypeTag[T]` / `WeakTypeTag[T]`, and what `T` is.
///
/// Both shapes a tag arrives in are recognised. Reached through the *pickle*
/// -- `c.WeakTypeTag[R]`, an alias on `blackbox.Context` -- it is a resolved
/// `Type::Class`. Reached through `install_classpath`'s pickle subset, which
/// records member types by simple name, `TypeTags#typeOf`'s implicit
/// parameter is an unresolved `Type::Named { name: "TypeTags$TypeTag" }`:
/// the class file `TypeTags$TypeTag.class` carries no `ScalaSignature` of its
/// own (a trait's nested class is pickled inside the trait), so nothing had
/// entered a symbol for it.
pub(crate) fn tag_request(st: &SymbolTable, pt: &Type) -> Option<(Tag, Type)> {
    let (name, args) = match pt {
        Type::Class { sym, args } => (st.get(*sym).jvm_name.clone(), args),
        Type::Named { name, args } => (name.clone(), args),
        _ => return None,
    };
    if args.len() != 1 {
        return None;
    }
    let tag = match name.as_str() {
        TYPE_TAG | "TypeTags$TypeTag" => Tag::Strong,
        WEAK_TYPE_TAG | "TypeTags$WeakTypeTag" => Tag::Weak,
        _ => return None,
    };
    Some((tag, args[0].clone()))
}

/// What the synthetic `TypeCreator.apply` evaluates to.
///
/// A tag for a *monomorphic* class is one `staticClass` call
/// ([`TagBody::StaticClass`]). Everything else is a composition, and each
/// piece is either built or named in a diagnostic -- never approximated.
#[derive(Clone)]
pub(crate) enum TagBody {
    /// `$m$untyped.staticClass("<name>").asType.toTypeConstructor`.
    StaticClass(String),
    /// `$m$untyped.universe.appliedType($m$untyped.staticClass("<name>"),
    /// List(<args>))` -- a type constructor at its arguments.
    ///
    /// nsc writes `internal.reificationSupport.TypeRef(thisPrefix(<owner>),
    /// <sym>, List(…))`; `appliedType(sym, args)` is the public spelling of
    /// the same `TypeRef`, since a symbol's `typeConstructor` *is*
    /// `TypeRef(owner.thisType, sym, Nil)`. `crates/cli/tests/engine.rs`
    /// compares the resulting `tpe` against the tag real scalac builds.
    Applied {
        class_name: String,
        args: Vec<TagBody>,
    },
    /// `<tag>.in($m$untyped).tpe` -- a type parameter whose tag is in scope,
    /// rebased into the mirror the creator is handed. This is what makes
    /// `c.Expr[F[E]]` inside `def impl[E](c: Context)(implicit e:
    /// c.WeakTypeTag[E])` work: `F[E]` is only knowable through `e`.
    FromTag(Box<Tree>),
}

/// The name `Mirror.staticClass` is given for `ty`, or why there is none.
///
/// Every accepted form is a *class* with no type arguments, which is exactly
/// what one `staticClass` call can rebuild. The rest are named, never guessed:
/// see the module comment.
pub(crate) fn static_class_name(st: &SymbolTable, ty: &Type) -> Result<String, String> {
    let named = |s: &str| Ok(s.to_string());
    match ty {
        Type::Boolean => named("scala.Boolean"),
        Type::Byte => named("scala.Byte"),
        Type::Short => named("scala.Short"),
        Type::Char => named("scala.Char"),
        Type::Int => named("scala.Int"),
        Type::Long => named("scala.Long"),
        Type::Float => named("scala.Float"),
        Type::Double => named("scala.Double"),
        Type::Unit => named("scala.Unit"),
        Type::Any => named("scala.Any"),
        Type::AnyVal => named("scala.AnyVal"),
        Type::Nothing => named("scala.Nothing"),
        Type::Null => named("scala.Null"),
        // `Predef.String` is an alias for `java.lang.String`; the tag nsc
        // builds spells the alias out with `selectType`, and both print
        // `String` and are `=:=`.
        Type::String => named("java.lang.String"),
        // An alias for `java.lang.Object`, not a class: `staticClass` would
        // throw at run time, so it is refused at compile time instead.
        Type::AnyRef => Err("`AnyRef`, which is an alias rather than a class".to_string()),
        Type::Class { sym, args } if args.is_empty() => static_class_of_sym(st, *sym),
        Type::Class { sym, .. } => Err(format!(
            "`{}`, a type constructor applied to type arguments",
            st.get(*sym).name
        )),
        Type::TypeParam(id) | Type::TypeMember(id) => Err(format!(
            "`{}`, an abstract type with no tag in scope",
            st.get(*id).name
        )),
        Type::ModuleRef(_) | Type::ThisType(_) | Type::SingleType { .. } => {
            Err(format!("`{}`, a singleton type", st.display_type(ty)))
        }
        Type::Refined { .. } => Err(format!("`{}`, a structural type", st.display_type(ty))),
        Type::Function { .. } | Type::Tuple(_) | Type::Array(_) => Err(format!(
            "`{}`, whose type arguments would have to be reified too",
            st.display_type(ty)
        )),
        other => Err(format!("`{}`", st.display_type(other))),
    }
}

/// The name `Mirror.staticClass` is given a class symbol, or why there is none.
///
/// `staticClass` walks packages and stops: a class nested in a class or an
/// object is reached by `selectType` on the enclosing symbol's info instead,
/// which is a second shape of creator body and not built here. `Nest$Inner` --
/// a `$` anywhere in the class file's simple name -- is exactly that case.
pub(crate) fn static_class_of_sym(st: &SymbolTable, sym: SymbolId) -> Result<String, String> {
    let s = st.get(sym);
    if !matches!(s.kind, SymKind::Class) {
        return Err(format!("`{}`, which is not a class", s.name));
    }
    let jvm = st.jvm_internal(sym);
    if jvm.is_empty() {
        return Err(format!("`{}`, which has no class name", s.name));
    }
    let simple = jvm.rsplit('/').next().unwrap_or(&jvm);
    if simple.contains('$') {
        return Err(format!(
            "`{}`, a class nested in a class or an object rather than a top-level one",
            s.name
        ));
    }
    Ok(jvm.replace('/', "."))
}

/// The reflection classes the materialiser needs symbols for.
///
/// None of them is named by the program being compiled -- `typeOf[Foo]`
/// mentions neither `Mirror` nor `TypeCreator` -- so each is loaded on
/// demand by `Check::reflect_class` rather than looked up and hoped for.
#[derive(Clone, Copy)]
pub(crate) struct TagClasses {
    /// `TypeTags$TypeTag` / `TypeTags$WeakTypeTag`.
    pub(crate) tag_cls: SymbolId,
    /// `scala.reflect.api.TypeTags`, which declares the companion's accessor.
    pub(crate) type_tags: SymbolId,
    pub(crate) mirror: SymbolId,
    pub(crate) creator: SymbolId,
}

/// Install `TypeTags#TypeTag$` / `TypeTags#WeakTypeTag$` -- the companion of
/// the tag trait -- and its `apply`, and point the universe's accessor at it.
///
/// The module has a class file (`TypeTags$TypeTag$.class`) but no pickle of
/// its own: a trait's nested object is pickled inside the enclosing
/// `TypeTags`, and `install_classpath` skips a class file with no
/// `ScalaSignature`. So nothing ever entered a symbol for it, the descriptor
/// `()Lscala/reflect/api/TypeTags$TypeTag$;` on `TypeTags#TypeTag` came back
/// as an unresolved `Type::Named`, and `u.TypeTag.apply` / `u.TypeTag.Int`
/// were "not a member of TypeTags$TypeTag$" (`docs/macros.md` §7.8, item 5).
///
/// `apply`'s erased descriptor is written out rather than derived: the Scala
/// signature is `apply[T](mirror1: Mirror[TypeTags.this.type], tpec1:
/// TypeCreator): TypeTag[T]`, whose first parameter is a singleton-typed
/// `Mirror` scala-rs has no way to spell. A method symbol whose `jvm_name`
/// starts with `(` is called with exactly that descriptor, which is how
/// `pickle_supply` installs library members too.
pub(crate) fn ensure_tag_module(
    st: &mut SymbolTable,
    tag: Tag,
    cls: TagClasses,
) -> Option<SymbolId> {
    let TagClasses {
        tag_cls,
        type_tags,
        mirror,
        creator,
    } = cls;
    let tag_jvm = tag.jvm().to_string();
    let module_jvm = format!("{tag_jvm}$");
    // Everything below happens once per tag, and `apply` is the record that
    // it did. (`resolve_named_tags` walks the whole symbol table, and a file
    // with many `typeOf[T]`s would otherwise pay for each one.)
    //
    // The *module class* alone is not that record: `PickleSupply::
    // install_nested_module` enters one for every nested `object` a pickle
    // declares, `TypeTags.TypeTag` included, and it deliberately installs no
    // members -- `apply`'s signature is one no pickle conversion can express.
    // Returning on the module's mere presence therefore left the tag
    // companion with no `apply` at all, depending only on whether some
    // earlier line in the file had written `u.TypeTag`.
    let existing = crate::classpath::find_by_jvm(st, &module_jvm);
    if let Some(id) = existing {
        if st
            .lookup_member(id, "apply")
            .into_iter()
            .any(|m| st.get(m).kind == SymKind::Method)
        {
            return Some(id);
        }
    }
    resolve_named_tags(st, tag, tag_cls);
    let simple = tag.simple();

    // Allocated ownerless and then re-owned: entering it in `TypeTags`'
    // member list would put a second `TypeTag` next to the accessor the class
    // file already declares, and member lookup would have to choose.
    let mcls = existing.unwrap_or_else(|| {
        let id = st.alloc(
            format!("{simple}$"),
            SymbolId::NONE,
            SymKind::ModuleClass,
            Flags::MODULE.with(Flags::FINAL),
            &module_jvm,
        );
        st.get_mut(id).owner = type_tags;
        st.get_mut(id).ty = Type::ModuleRef(id);
        st.get_mut(id).parents = vec![Type::AnyRef];
        id
    });

    let ap = st.alloc("apply", mcls, SymKind::Method, Flags::EMPTY, "");
    let t = st.alloc("T", ap, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(t).ty = Type::TypeParam(t);
    st.get_mut(ap).tparams = vec![t];
    st.get_mut(ap).ty = Type::Method {
        paramss: vec![vec![
            Type::Class {
                sym: mirror,
                args: vec![],
            },
            Type::Class {
                sym: creator,
                args: vec![],
            },
        ]],
        ret: Box::new(Type::Class {
            sym: tag_cls,
            args: vec![Type::TypeParam(t)],
        }),
    };
    st.get_mut(ap).jvm_name = format!("(L{MIRROR};L{TYPE_CREATOR};)L{tag_jvm};");

    // `trait TypeTags { object TypeTag ... }` compiles to an interface method
    // `TypeTag()Lscala/reflect/api/TypeTags$TypeTag$;`. Whether the typer has
    // it depends on which door `TypeTags` came in by: read as a *class file*
    // it is among the methods, read from its *pickle* -- which is what
    // happens when nothing on the classpath scan named it -- module members
    // are not among the shapes `complete_named` installs, and the accessor is
    // missing entirely ("value TypeTag is not a member of JavaUniverse").
    // Declared here, with the descriptor written out, so the call is the same
    // either way.
    //
    // Unless one already points at this module class from somewhere else:
    // `PickleSupply::install_nested_module` supplies the accessor for every
    // nested `object` a pickle declares, and it installs it on the *receiver*
    // class the lookup started from rather than on `TypeTags`. Adding a
    // second one here would leave `u.TypeTag` with two nullary candidates
    // that mean the same thing.
    let already = st.symbols.iter().any(|s| {
        s.name == simple
            && matches!(s.kind, SymKind::Method | SymKind::Term)
            && matches!(&s.ty, Type::Method { paramss, ret }
                if paramss.iter().all(|c| c.is_empty())
                    && matches!(**ret, Type::ModuleRef(m) if m == mcls))
    });
    if !already {
        let acc = st.alloc(
            simple,
            type_tags,
            SymKind::Method,
            Flags::EMPTY,
            format!("()L{module_jvm};"),
        );
        st.get_mut(acc).ty = Type::Method {
            paramss: Vec::new(),
            ret: Box::new(Type::ModuleRef(mcls)),
        };
    }

    // The accessor the universe declares (`def TypeTag: TypeTags$TypeTag$`)
    // now has somewhere to point. Every copy of it is repaired: the member is
    // read from the class file onto whichever class the lookup reached.
    // The name the descriptor gave it: `TypeTags$TypeTag$`, the class file's
    // simple name, not the source's `TypeTag`.
    let unresolved = module_jvm
        .rsplit('/')
        .next()
        .unwrap_or(&module_jvm)
        .to_string();
    let module_ref = Type::ModuleRef(mcls);
    for i in 0..st.symbols.len() {
        let id = SymbolId(i as u32);
        if st.get(id).name != simple {
            continue;
        }
        let ty = st.get(id).ty.clone();
        let repaired = replace_named(&ty, &unresolved, &module_ref);
        if repaired != ty {
            st.get_mut(id).ty = repaired;
        }
    }
    Some(mcls)
}

/// Point every `Type::Named { name: "TypeTags$TypeTag" }` at the real class.
///
/// `install_classpath` reads `TypeTags`' pickle subset, which names member
/// types by their simple name, and `TypeTags$TypeTag` is not a name anything
/// had entered -- so `def typeOf[T](implicit ttag: TypeTag[T])` came out with
/// an *unresolved* parameter type. That is what the "could not find implicit
/// value of type TypeTags$TypeTag[Foo]" report was about, and it is also what
/// erasure would have written the call's descriptor from. Now that the class
/// symbol exists, every mention of it is repaired.
fn resolve_named_tags(st: &mut SymbolTable, tag: Tag, tag_cls: SymbolId) {
    let unresolved = format!("TypeTags${}", tag.simple());
    let real = Type::Class {
        sym: tag_cls,
        args: vec![],
    };
    for i in 0..st.symbols.len() {
        let id = SymbolId(i as u32);
        let ty = st.get(id).ty.clone();
        let repaired = replace_named(&ty, &unresolved, &real);
        if repaired != ty {
            st.get_mut(id).ty = repaired;
        }
    }
}

/// Substitute `Type::Named { name, args }` for `to` carrying the same `args`.
fn replace_named(ty: &Type, name: &str, to: &Type) -> Type {
    match ty {
        Type::Named { name: n, args } if n == name => {
            let args: Vec<Type> = args.iter().map(|a| replace_named(a, name, to)).collect();
            match to {
                Type::Class { sym, .. } => Type::Class { sym: *sym, args },
                other if args.is_empty() => other.clone(),
                other => Type::Applied {
                    ctor: Box::new(other.clone()),
                    args,
                },
            }
        }
        Type::Named { name: n, args } => Type::Named {
            name: n.clone(),
            args: args.iter().map(|a| replace_named(a, name, to)).collect(),
        },
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(|a| replace_named(a, name, to)).collect(),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|c| c.iter().map(|p| replace_named(p, name, to)).collect())
                .collect(),
            ret: Box::new(replace_named(ret, name, to)),
        },
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| replace_named(p, name, to)).collect(),
            ret: Box::new(replace_named(ret, name, to)),
        },
        Type::ByName(t) => Type::ByName(Box::new(replace_named(t, name, to))),
        Type::Repeated(t) => Type::Repeated(Box::new(replace_named(t, name, to))),
        Type::Array(t) => Type::Array(Box::new(replace_named(t, name, to))),
        other => other.clone(),
    }
}

/// Builds the materialisation tree; see the module comment for its shape.
pub(crate) struct Materialiser<'a> {
    /// The expression naming the universe, already typed and cloned in.
    pub(crate) universe: &'a Tree,
    /// Name of the synthetic `TypeCreator` subclass, unique in the run.
    pub(crate) creator_name: String,
    /// The `Type` the tag stands for, spliced into `TypeTag.apply[T]`.
    pub(crate) arg: Type,
    /// How the creator rebuilds the type.
    pub(crate) body: TagBody,
    /// The tag companion's simple name: `TypeTag` or `WeakTypeTag`.
    pub(crate) tag_name: String,
    /// `scala.reflect.api.Mirror`, for the cast on the mirror argument.
    pub(crate) mirror_ty: Type,
    /// `scala.reflect.api.Types.TypeApi`, the creator's erased result.
    pub(crate) type_api: Type,
    pub(crate) span: Span,
}

impl Materialiser<'_> {
    pub(crate) fn build(&self) -> Tree {
        let creator = self.creator_class();
        let call = self.node(TreeKind::Apply {
            fun: Box::new(self.node(TreeKind::TypeApply {
                fun: Box::new(
                    self.select(self.select(self.universe.clone(), &self.tag_name), "apply"),
                ),
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
    /// The cast is not decoration. `rootMirror`'s type is the universe's
    /// abstract member `Mirror`, whose upper bound scala-rs reads from the
    /// pickle as `JavaMirror` and no further -- `JavaMirror extends
    /// api.Mirror[self.type]` is a parent `conv_upper_bound` drops, because
    /// the singleton argument does not convert. So the value really is a
    /// `Mirror` and the typer cannot see it; the cast says so, and erases to
    /// a `checkcast` that always succeeds.
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

    /// ```text
    /// final class <creator_name> extends _root_.scala.reflect.api.TypeCreator {
    ///   def apply[U <: _root_.scala.reflect.api.Universe with Singleton](
    ///       $m$untyped: _root_.scala.reflect.api.Mirror[U]): U#Type =
    ///     $m$untyped.staticClass("<class_name>").asType.toTypeConstructor
    /// }
    /// ```
    fn creator_class(&self) -> Tree {
        let param = self.node(TreeKind::ValDef {
            mods: Modifiers {
                flags: Flags::PARAM,
                ..Modifiers::default()
            },
            name: "$m$untyped".to_string(),
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
        let body = self.rebuild(&self.body);
        let apply = self.node(TreeKind::DefDef {
            mods: Modifiers::default(),
            name: "apply".to_string(),
            tparams: vec![tparam],
            vparamss: vec![vec![param]],
            // nsc writes `U#Type` and erases it to `Types$TypeApi`, the
            // bound of the universe's abstract `type Type`. scala-rs erases
            // an abstract type member to `Object` (`erasure::erase_ty`), and
            // `TypeCreator.apply` is *abstract*: a descriptor ending in
            // `Object` overrides nothing, and the JVM answers the first
            // `tag.tpe` with `AbstractMethodError`. Writing the bound out is
            // what makes the override real.
            tpt: Box::new(self.resolved_type(self.type_api.clone())),
            rhs: Box::new(body),
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
                parents: vec![self.api_type("TypeCreator")],
                self_name: None,
                self_tpt: None,
                body: vec![apply],
                span: self.span,
            },
        })
    }

    /// The creator's body: how one type is rebuilt inside `$m$untyped`.
    fn rebuild(&self, body: &TagBody) -> Tree {
        match body {
            TagBody::StaticClass(name) => self.select(
                self.select(self.static_class(name), "asType"),
                "toTypeConstructor",
            ),
            TagBody::Applied { class_name, args } => {
                let list = self.node(TreeKind::Apply {
                    fun: Box::new(self.immutable_list()),
                    args: args.iter().map(|a| self.rebuild(a)).collect(),
                });
                self.node(TreeKind::Apply {
                    fun: Box::new(self.select(self.mirror_universe(), "appliedType")),
                    args: vec![self.static_class(class_name), list],
                })
            }
            // `.in` rebases the tag into the mirror this creator was handed,
            // the way nsc's own reifier writes `e.in($m).tpe`. Without it the
            // type would be the one from the tag's *original* universe.
            TagBody::FromTag(tag) => self.select(
                self.node(TreeKind::Apply {
                    fun: Box::new(self.select((**tag).clone(), "in")),
                    args: vec![self.untyped_mirror()],
                }),
                "tpe",
            ),
        }
    }

    fn untyped_mirror(&self) -> Tree {
        self.node(TreeKind::Ident {
            name: "$m$untyped".to_string(),
        })
    }

    /// `$m$untyped.staticClass("<name>")`.
    fn static_class(&self, name: &str) -> Tree {
        self.node(TreeKind::Apply {
            fun: Box::new(self.select(self.untyped_mirror(), "staticClass")),
            args: vec![self.node(TreeKind::Literal {
                lit: Lit::String(name.to_string()),
            })],
        })
    }

    /// `$m$untyped.universe` -- the universe the tag is being read in.
    fn mirror_universe(&self) -> Tree {
        self.select(self.untyped_mirror(), "universe")
    }

    /// `scala.collection.immutable.List`, spelled out: the creator's body is
    /// typed in the macro implementation's scope, where `import
    /// c.universe._` may have brought in another `List`.
    fn immutable_list(&self) -> Tree {
        let scala = self.node(TreeKind::Ident {
            name: "scala".to_string(),
        });
        let coll = self.select(scala, "collection");
        let imm = self.select(coll, "immutable");
        self.select(imm, "List")
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

    /// A type tree that is already a type. `tree_to_type` hands back `ty`
    /// unchanged for this marker, the way nsc's `TypeTree(tp)` does -- the
    /// type is one the typer computed, and re-deriving it from a name would
    /// need a path the source may not have (`scala.reflect.api.Mirror` is not
    /// imported at the use site).
    fn resolved_type(&self, ty: Type) -> Tree {
        let mut t = self.node(TreeKind::Ident {
            name: RESOLVED_TYPE.to_string(),
        });
        t.ty = ty;
        t
    }

    /// `scala.reflect.api.<name>`.
    ///
    /// Spelled out rather than left to whatever `import <universe>._` brought
    /// in: the universe offers *abstract type members* called `Mirror` and
    /// `Type`, and an unqualified `Mirror` picks those up instead of the
    /// `scala.reflect.api.Mirror[U]` the creator's parameter needs.
    fn api_type(&self, name: &str) -> Tree {
        let scala = self.node(TreeKind::Ident {
            name: "scala".to_string(),
        });
        let reflect = self.select(scala, "reflect");
        let api = self.select(reflect, "api");
        self.select(api, name)
    }

    /// `scala.<name>`.
    fn scala_type(&self, name: &str) -> Tree {
        let scala = self.node(TreeKind::Ident {
            name: "scala".to_string(),
        });
        self.select(scala, name)
    }
}

/// The name of the marker `Ident` that stands for an already-resolved type,
/// carried in the tree's `ty`. Recognised by `Check::tree_to_type`.
pub(crate) const RESOLVED_TYPE: &str = "$resolvedType";
