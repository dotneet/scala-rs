//! `scala.<:<` / `scala.=:=` and the handful of standard members that lean on
//! them (`Option.orNull`, `Iterable`/`IterableOnce.foreach`).
//!
//! Kept in its own module (called from `prelude.rs` with a single line) so
//! parallel prelude slices don't collide on the same lines of that file.
//!
//! Verified against the real jar (`/tmp/scala-rs-lib/scala-library-2.13.16.jar`,
//! javap'd directly) rather than against idealized nsc source, since dual-run
//! bytecode has to link against what's actually in that jar:
//!   - `scala.<:<[-From, +To]` (JVM `scala/$less$colon$less`) extends
//!     `Function1[From, To]` and declares a real `apply(From): To`.
//!   - `scala.=:=[From, To]` (JVM `scala/$eq$colon$eq`) extends `<:<[From, To]`.
//!   - The *only* generic implicit witness that exists at the JVM level is
//!     `<:<.refl[A]: A =:= A` on the `<:<` companion (`scala/$less$colon$less$`).
//!     There is no `scala/$eq$colon$eq$` companion classfile in 2.13.16 at all
//!     (so `=:=.tpEquals` doesn't exist to model), and `Predef.$conforms[A]`
//!     itself is erased to return `Function1[A, A]`, not `<:<[A, A]` — it is
//!     *not* usable as an `A <:< B` witness in real Scala either; real code
//!     goes through `<:<.refl`. We mirror that: `$conforms` is declared with
//!     its real signature (for source-compat / explicit calls) and `refl` is
//!     flagged `implicit`, so the ordinary implicit search finds it. Fitting
//!     it to the wanted type is the general polymorphic case
//!     (`implicits.rs::implicit_solve`): `refl`'s own `[A]` binds from the
//!     `From` position of `From <:< To`, and `<:<`'s declared variance
//!     (`-From, +To`) then makes `A =:= A` conform to `A <:< To` exactly when
//!     `A <: To` — the same derivation real scalac performs.
//!
//! Gated behind `library_abi`, exactly like `Either`/`Try`/`Using`: without a
//! real scala-library on the classpath there is no private-runtime classfile
//! to back these with, so `--no-scala-library` mode simply never installs
//! `<:<`/`=:=`, giving a genuine `not found: type <:<` diagnostic instead of a
//! silently-accepted stub.

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

fn class(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parents: &[Type],
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Class, Flags::FINAL, jvm);
    st.get_mut(id).parents = parents.to_vec();
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}

fn module(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    let cls = st.alloc(
        format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        jvm,
    );
    let m = st.alloc(name, owner, SymKind::Module, Flags::MODULE, jvm);
    st.get_mut(m).ty = Type::ModuleRef(cls);
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    m
}

fn type_param(st: &mut SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    id
}

fn method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
    intrinsic: Intrinsic,
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::FINAL, "");
    let paramss = if params.is_empty() {
        Vec::new()
    } else {
        vec![params]
    };
    st.get_mut(id).ty = Type::Method {
        paramss,
        ret: Box::new(ret),
    };
    st.get_mut(id).intrinsic = intrinsic;
    id
}

/// Called once from `install_prelude` *after* `Predef` exists (`st.predef`
/// is set partway through that function), so this can't live inside the
/// earlier `if library_abi { ... }` block that installs `Either`/`Try`/etc. —
/// it gates itself on `library_abi` instead.
pub fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    // `sealed abstract class <:<[-From, +To] extends (From => To)`.
    let less = crate::classpath::find_by_jvm(st, "scala/$less$colon$less").unwrap_or_else(|| {
        let id = class(
            st,
            st.scala_pkg,
            "<:<",
            "scala/$less$colon$less",
            &[Type::AnyRef],
        );
        let from = type_param(st, id, "From");
        st.get_mut(from).flags = st.get(from).flags.with(Flags::CONTRAVARIANT);
        let to = type_param(st, id, "To");
        st.get_mut(to).flags = st.get(to).flags.with(Flags::COVARIANT);
        st.get_mut(id).tparams = vec![from, to];
        st.get_mut(id).parents = vec![
            Type::Function {
                params: vec![Type::TypeParam(from)],
                ret: Box::new(Type::TypeParam(to)),
            },
            Type::AnyRef,
        ];
        let apply = method(
            st,
            id,
            "apply",
            vec![Type::TypeParam(from)],
            Type::TypeParam(to),
            Intrinsic::None,
        );
        let _ = apply;
        id
    });

    // `sealed abstract class =:=[From, To] extends (From <:< To)`.
    let eq = crate::classpath::find_by_jvm(st, "scala/$eq$colon$eq").unwrap_or_else(|| {
        let id = class(st, st.scala_pkg, "=:=", "scala/$eq$colon$eq", &[]);
        let from = type_param(st, id, "From");
        let to = type_param(st, id, "To");
        st.get_mut(id).tparams = vec![from, to];
        st.get_mut(id).parents = vec![Type::Class {
            sym: less,
            args: vec![Type::TypeParam(from), Type::TypeParam(to)],
        }];
        // `lookup_member`'s parent walk *would* find `<:<::apply` here, but
        // member selection substitutes using the referenced symbol's own
        // owner's tparams, not through a chain of distinct parent tparam
        // identities — inheriting it structurally resolves `apply` at
        // `<:<`'s own (different) `From`/`To` `SymbolId`s, not `=:='s. Same
        // reason `PartialFunction` (below `add_partial_function`) declares
        // its own `apply` instead of leaning on its `Function1` parent.
        method(
            st,
            id,
            "apply",
            vec![Type::TypeParam(from)],
            Type::TypeParam(to),
            Intrinsic::None,
        );
        id
    });

    // `object <:<` — real 2.13.16 companion, JVM `scala/$less$colon$less$`.
    // `refl[A]: A =:= A` is the only witness the real jar backs (there is no
    // `object =:=` classfile to hang a `tpEquals` off of).
    if crate::classpath::find_by_jvm(st, "scala/$less$colon$less$").is_none() {
        let less_mod = module(st, st.scala_pkg, "<:<", "scala/$less$colon$less$");
        let less_cls = st.module_class_of(less_mod);
        // `refl` is the discoverable witness: implicit search unifies its own
        // `[A]` against the wanted `From <:< To` / `From =:= To`
        // (`implicits.rs::implicit_solve`), so no dedicated fallback is needed.
        let refl = st.alloc(
            "refl",
            less_cls,
            SymKind::Method,
            Flags::FINAL.with(Flags::IMPLICIT),
            "",
        );
        let ra = type_param(st, refl, "A");
        st.get_mut(refl).tparams = vec![ra];
        st.get_mut(refl).ty = Type::Method {
            paramss: vec![vec![]],
            ret: Box::new(Type::Class {
                sym: eq,
                args: vec![Type::TypeParam(ra), Type::TypeParam(ra)],
            }),
        };
    }

    // `Predef.$conforms[A]: A => A`, real erased signature (`Function1[A, A]`,
    // not `<:<[A, A]` — see module doc comment). Kept for source/bytecode
    // compat with code that calls it explicitly; it is deliberately *not*
    // an implicit `<:<` witness (its result type isn't one), so implicit
    // search for `A <:< B` lands on `<:<.refl` like real scalac's does.
    install_conforms_member(st);

    // `Option[A].orNull(implicit ev: Null <:< A): A`.
    // nsc's real signature introduces a fresh `A1 >: A`; we reuse `Option`'s
    // own (already-concrete-at-any-call-site) type param instead, since this
    // typer's implicit-clause auto-fill (`adapt_implicit_apply` in check.rs)
    // only fires for a symbol with *no* unsubstituted type params of its own.
    // The two coincide for every real use (`Option[String].orNull` etc.); the
    // only thing lost is the (rarely used) upcast to an unrelated nullable
    // supertype, which would fail to find a witness under the invariant form
    // anyway only when `A` itself isn't nullable — exactly when real scalac
    // rejects `orNull` too (e.g. `Option[Int].orNull`).
    let o = st.option_sym;
    if st
        .get(o)
        .members
        .iter()
        .all(|&m| st.get(m).name != "orNull")
    {
        let ta = Type::TypeParam(st.get(o).tparams[0]);
        let or_null = st.alloc("orNull", o, SymKind::Method, Flags::FINAL, "");
        let ev = st.alloc(
            "ev",
            or_null,
            SymKind::Term,
            Flags::PARAM.with(Flags::IMPLICIT),
            "",
        );
        let ev_ty = Type::Class {
            sym: less,
            args: vec![Type::Null, ta.clone()],
        };
        st.get_mut(ev).ty = ev_ty.clone();
        st.get_mut(or_null).params = vec![ev];
        st.get_mut(or_null).paramss = vec![vec![ev]];
        st.get_mut(or_null).ty = Type::Method {
            paramss: vec![vec![ev_ty]],
            ret: Box::new(ta),
        };
    }

    // `Iterable`/`IterableOnce.foreach` — both ifaces exist (created lazily by
    // ArrayOps' `flatMap`/`zip` overloads) but neither declared any members of
    // its own; `foreach` is the one slick actually calls through a
    // statically-`Iterable`-typed value.
    for jvm in ["scala/collection/Iterable", "scala/collection/IterableOnce"] {
        let Some(id) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        if st.get(id).tparams.is_empty() {
            continue;
        }
        if st
            .get(id)
            .members
            .iter()
            .any(|&m| st.get(m).name == "foreach")
        {
            continue;
        }
        let a = Type::TypeParam(st.get(id).tparams[0]);
        method(
            st,
            id,
            "foreach",
            vec![Type::Function {
                params: vec![a],
                ret: Box::new(Type::Any),
            }],
            Type::Unit,
            Intrinsic::None,
        );
    }

    // `List <: Iterable` (real nsc hierarchy: `List extends ... with Iterable[A]`).
    // Without this, a `List[Int]` value can never actually reach the new
    // `Iterable[Int].foreach` above — nothing could dual-run-exercise it —
    // and slick's "value foreach is not a member of Iterable[T]" is exactly a
    // static-`Iterable[T]`-typed slot being handed a concrete collection.
    // Mirrors the existing `List <: IterableOnce` wiring right above this
    // function (`add_array_ops_zip`, prelude.rs) which does the same push.
    if let Some(iterable) = crate::classpath::find_by_jvm(st, "scala/collection/Iterable") {
        if let Some(la) = st.get(st.list_sym).tparams.first().copied() {
            let parent = Type::Class {
                sym: iterable,
                args: vec![Type::TypeParam(la)],
            };
            if !st
                .get(st.list_sym)
                .parents
                .iter()
                .any(|p| matches!(p, Type::Class { sym, .. } if *sym == iterable))
            {
                st.get_mut(st.list_sym).parents.push(parent);
            }
        }
    }

    // Unqualified resolution (`resolve_type_name` in check.rs) only ever
    // consults `SymbolTable::lookup`, i.e. the scope chain — it does *not*
    // fall back to scanning `scala.collection`/`scala.collection.immutable`
    // by source name. `<:<`/`=:=` happen to get imported for free (their
    // owner is `scala_pkg`, swept up by `import_members(st, st.scala_pkg)`
    // a few lines above this call in `install_prelude`), but `Iterable`/
    // `IterableOnce` live under a *different* package symbol (`coll`, from
    // `ensure_package(st, "scala/collection")` in `add_array_ops_*`) that
    // nothing imports — so without an explicit entry here, plain `Iterable`
    // in source falls through to `expose_unqualified`'s classpath probe,
    // which only ever tries a *root-level* class file (`Iterable.class`,
    // not `scala/collection/Iterable.class`), silently fails, and leaves
    // `Iterable[Int]` an unresolved `Type::Named` (surfacing later as
    // "Iterable does not take type parameters"). Entering all four by hand
    // here — after `install_prelude`'s own scope setup, so nothing above
    // can shadow them and nothing below overwrites them — is the same fix
    // `install_prelude` already applies to `Ordered` for the identical
    // reason (`math` isn't `scala_pkg` either).
    for (name, jvm) in [
        ("<:<", "scala/$less$colon$less"),
        ("=:=", "scala/$eq$colon$eq"),
        ("Iterable", "scala/collection/Iterable"),
        ("IterableOnce", "scala/collection/IterableOnce"),
    ] {
        if let Some(id) = crate::classpath::find_by_jvm(st, jvm) {
            st.enter_in_current(name, id);
        }
    }
}

fn install_conforms_member(st: &mut SymbolTable) {
    // `add_predef_members` (prelude.rs) already ran and synced its members
    // from the module *class* onto `st.predef` (the module value) once; we
    // run after that, so we must do our own one-symbol sync instead of
    // re-copying the whole class member list (that would duplicate every
    // earlier Predef member in `st.predef.members`).
    if st
        .get(st.predef)
        .members
        .iter()
        .any(|&m| st.get(m).name == "$conforms")
    {
        return;
    }
    let predef_cls = st.module_class_of(st.predef);
    let conforms = st.alloc(
        "$conforms",
        predef_cls,
        SymKind::Method,
        Flags::FINAL.with(Flags::IMPLICIT),
        "",
    );
    let a = type_param(st, conforms, "A");
    st.get_mut(conforms).tparams = vec![a];
    st.get_mut(conforms).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Function {
            params: vec![Type::TypeParam(a)],
            ret: Box::new(Type::TypeParam(a)),
        }),
    };
    st.get_mut(st.predef).members.push(conforms);
}
