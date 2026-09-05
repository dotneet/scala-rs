//! The infix extractor objects of the collections library: `+:`, `:+` and
//! `#::`.
//!
//! `case h :: t` has always worked, because `scala.::` is a *case class* and
//! the pattern goes through the constructor-pattern path. Its three siblings
//! are plain objects with an `unapply`, and none of them was in the symbol
//! table at all, so `case h +: t` / `case t :+ h` / `case h #:: t` reported
//! `not found: extractor +:` and left every name the pattern bound unresolved
//! -- six `+:` patterns and eight `#::` ones in cats alone, plus eleven
//! cascaded "not found: value tail / rest / a".
//!
//! What real scalac 2.13.16 resolves them to (`-Xprint:typer` on a file that
//! uses all three, then `javap` on the result):
//!
//! ```text
//! scala/collection/package$$plus$colon$.unapply:(Lscala/collection/SeqOps;)Lscala/Option;
//! scala/collection/package$$colon$plus$.unapply:(Lscala/collection/SeqOps;)Lscala/Option;
//! scala/package$$hash$colon$colon$.unapply:(Lscala/collection/immutable/LazyList;)Lscala/Option;
//! scala/package$$hash$colon$colon$.unapply:(Lscala/collection/immutable/Stream;)Lscala/Option;
//! ```
//!
//! The first two are reached in source as `scala.package.+:` / `scala.package.:+`
//! (aliases of `scala.collection.+:` / `scala.collection.:+`), the third is an
//! object nested in `scala.package` itself -- and it is *overloaded*, one
//! alternative per lazy sequence type. `Typer::find_unapply` picks between them
//! by the scrutinee.
//!
//! `+:` and `:+` are declared here as
//!
//! ```scala
//! def unapply[A, C <: Seq[A]](t: C): Option[(A, C)]
//! ```
//!
//! rather than with scalac's `C with SeqOps[A, CC, C]`, which needs a
//! compound type this symbol table cannot spell. The `C` matters: cats matches
//! an `ArraySeq[A]` with `case _ +: rest` and hands `rest` back to a method
//! that takes an `ArraySeq[A]`, so an extractor that yielded `Seq[A]` would
//! trade one error for another. `A` is then determined through `C`'s *bound*,
//! which is what `Typer::subst_unapply_tparams` learned to do for this.
//!
//! The erased descriptor is `SeqOps`, which a bare `C` does not produce;
//! `crates/backend/src/gen_invoke.rs` names it for these two owners, exactly
//! as it already does for the `unapplySeq` of the sequence factories.
//!
//! **The private runtime (`--no-scala-library`) has none of these objects**, so
//! nothing is declared without the jar and the pattern keeps reporting `not
//! found: extractor +:` there.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::prelude::{ctor_field, module, type_param};
use crate::symbol::{SymKind, SymbolTable};

/// `scala.collection.+:`, the JVM class its `unapply` is declared on.
pub(crate) const PLUS_COLON_MODULE: &str = "scala/collection/package$$plus$colon$";
/// `scala.collection.:+`.
pub(crate) const COLON_PLUS_MODULE: &str = "scala/collection/package$$colon$plus$";
/// `scala.#::`.
pub(crate) const HASH_COLON_COLON_MODULE: &str = "scala/package$$hash$colon$colon$";

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let Some(seq) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq") else {
        return;
    };
    for (name, jvm, head_first) in [
        ("+:", PLUS_COLON_MODULE, true),
        (":+", COLON_PLUS_MODULE, false),
    ] {
        let m = module(st, coll, name, jvm);
        let mcls = st.module_class_of(m);
        add_seq_unapply(st, mcls, seq, head_first);
        st.enter_in_current(name, m);
    }
    let m = module(st, st.scala_pkg, "#::", HASH_COLON_COLON_MODULE);
    let mcls = st.module_class_of(m);
    if let Some(cls) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/LazyList") {
        add_cons_unapply(st, mcls, cls);
    }
    st.enter_in_current("#::", m);
    add_deferrer(st, "scala/collection/immutable/LazyList", true);
    // `Stream`'s half waits for `ensure_stream_support`; see there.
}

/// `LazyList.Deferrer` / `Stream.Deferrer`: the value class that carries the
/// cons operators `#::` and `#:::`.
///
/// `a #:: xs` is right-associative, so it selects `#::` on `xs` -- which has
/// no such member. What makes it work in the library is
///
/// ```scala
/// object LazyList {
///   implicit def toDeferrer[A](l: => LazyList[A]): Deferrer[A]
/// }
/// final class Deferrer[A](private val l: () => LazyList[A]) extends AnyVal {
///   def #:: [B >: A](elem: => B): LazyList[B]
///   def #:::[B >: A](prefix: LazyList[B]): LazyList[B]
/// }
/// ```
///
/// and the *by-name* parameter of the conversion is the whole point: the tail
/// may not be forced, or `lazy val ones: LazyList[Int] = 1 #:: ones` would not
/// terminate. Declaring it this shape lets the ordinary machinery do the work
/// -- `Typer::adapt` wraps an argument in a `Function0` when the parameter is
/// `=> T`, and the backend already routes a value class's members through
/// `<owner>$.<name>$extension` with the underlying value as the first
/// argument, which is exactly the call scalac emits:
///
/// ```text
/// LazyList$Deferrer$.$hash$colon$colon$extension:(Lscala/Function0;Lscala/Function0;)Lscala/collection/immutable/LazyList;
/// Stream$Deferrer$.$hash$colon$colon$extension:(Lscala/Function0;Ljava/lang/Object;)Lscala/collection/immutable/Stream;
/// ```
///
/// The two differ in exactly one place, which is what `lazy_elem` says:
/// `Stream`'s `#::` takes its head **strictly** (`elem: B`), `LazyList`'s does
/// not.
fn add_deferrer(st: &mut SymbolTable, coll_jvm: &str, lazy_elem: bool) {
    let Some(coll) = crate::classpath::find_by_jvm(st, coll_jvm) else {
        return;
    };
    let Some(mcls) = crate::classpath::find_by_jvm(st, &format!("{coll_jvm}$")) else {
        return;
    };
    let mcls = if st.get(mcls).kind == SymKind::Module {
        st.module_class_of(mcls)
    } else {
        mcls
    };
    if mcls.is_none() || st.get(mcls).kind != SymKind::ModuleClass {
        return;
    }
    // Idempotent: `ensure_stream_support` may call this more than once.
    if !st.lookup_member(mcls, "toDeferrer").is_empty() {
        return;
    }
    let d = st.alloc(
        "Deferrer",
        mcls,
        SymKind::Class,
        Flags::FINAL,
        format!("{coll_jvm}$Deferrer"),
    );
    let a = type_param(st, d, "A");
    st.get_mut(d).tparams = vec![a];
    st.get_mut(d).parents = vec![Type::AnyVal];
    st.get_mut(d).ty = Type::Class {
        sym: d,
        args: vec![],
    };
    let coll_of = |t: Type| Type::Class {
        sym: coll,
        args: vec![t],
    };
    let l = ctor_field(
        st,
        d,
        "l",
        Type::Function {
            params: vec![],
            ret: Box::new(coll_of(Type::TypeParam(a))),
        },
    );
    st.get_mut(d).ctor_fields = vec![l];
    for (name, is_prefix) in [("#::", false), ("#:::", true)] {
        let m = st.alloc(name, d, SymKind::Method, Flags::FINAL, "");
        let b = type_param(st, m, "B");
        st.get_mut(b).bound_lo = Some(Type::TypeParam(a));
        st.get_mut(m).tparams = vec![b];
        let param = if is_prefix {
            coll_of(Type::TypeParam(b))
        } else if lazy_elem {
            Type::ByName(Box::new(Type::TypeParam(b)))
        } else {
            Type::TypeParam(b)
        };
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![param]],
            ret: Box::new(coll_of(Type::TypeParam(b))),
        };
    }
    let td = st.alloc(
        "toDeferrer",
        mcls,
        SymKind::Method,
        Flags::IMPLICIT.with(Flags::FINAL),
        "",
    );
    let ta = type_param(st, td, "A");
    st.get_mut(td).tparams = vec![ta];
    st.get_mut(td).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(coll_of(Type::TypeParam(ta))))]],
        ret: Box::new(Type::Class {
            sym: d,
            args: vec![Type::TypeParam(ta)],
        }),
    };
}

/// Everything `#::` needs for `Stream`, added the first time source names a
/// `Stream` in one of the two positions that want it.
///
/// It cannot go in with the `LazyList` half. `Stream` has no hand-written
/// prelude declaration -- it is a package-object alias, and its class reaches
/// the symbol table only when source names it -- and stubbing it during the
/// prelude would be worse than not having it: `give_stub_its_kinds`
/// (`pickle_supply.rs`) leaves every `scala/*` symbol allocated before
/// `prelude_end` alone, so the stub would keep zero type parameters for the
/// whole run and `type Stream[+A] = scala.collection.immutable.Stream[A]`
/// would stop converting -- an alias that used to work.
///
/// Idempotent, and a no-op for any class that is not `Stream`.
pub(crate) fn ensure_stream_support(st: &mut SymbolTable, cls: SymbolId) {
    if cls.is_none() || st.get(cls).jvm_name != "scala/collection/immutable/Stream" {
        return;
    }
    if let Some(m) = crate::classpath::find_by_jvm(st, HASH_COLON_COLON_MODULE) {
        let mcls = if st.get(m).kind == SymKind::Module {
            st.module_class_of(m)
        } else {
            m
        };
        let already = st.lookup_member(mcls, "unapply").into_iter().any(|u| {
            matches!(&st.get(u).ty, Type::Method { paramss, .. }
                if paramss.first().and_then(|ps| ps.first())
                    .and_then(|p| st.class_sym_of(p)) == Some(cls))
        });
        if !already {
            add_cons_unapply(st, mcls, cls);
        }
    }
    add_deferrer(st, "scala/collection/immutable/Stream", false);
}

/// `def unapply[A, C <: Seq[A]](t: C): Option[(A, C)]` (`+:`), or
/// `Option[(C, A)]` (`:+`, whose head is the *last* element).
fn add_seq_unapply(st: &mut SymbolTable, mcls: SymbolId, seq: SymbolId, head_first: bool) {
    let id = st.alloc("unapply", mcls, SymKind::Method, Flags::FINAL, "");
    let a = type_param(st, id, "A");
    let c = type_param(st, id, "C");
    st.get_mut(c).bound_hi = Some(Type::Class {
        sym: seq,
        args: vec![Type::TypeParam(a)],
    });
    st.get_mut(id).tparams = vec![a, c];
    let payload = if head_first {
        vec![Type::TypeParam(a), Type::TypeParam(c)]
    } else {
        vec![Type::TypeParam(c), Type::TypeParam(a)]
    };
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(c)]],
        ret: Box::new(Type::Class {
            sym: st.option_sym,
            args: vec![Type::Tuple(payload)],
        }),
    };
}

/// `def unapply[A](s: LazyList[A]): Option[(A, LazyList[A])]`, once per lazy
/// sequence class. Two overloads, as in the library.
fn add_cons_unapply(st: &mut SymbolTable, mcls: SymbolId, cls: SymbolId) {
    let id = st.alloc("unapply", mcls, SymKind::Method, Flags::FINAL, "");
    let a = type_param(st, id, "A");
    st.get_mut(id).tparams = vec![a];
    let coll = Type::Class {
        sym: cls,
        args: vec![Type::TypeParam(a)],
    };
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![coll.clone()]],
        ret: Box::new(Type::Class {
            sym: st.option_sym,
            args: vec![Type::Tuple(vec![Type::TypeParam(a), coll])],
        }),
    };
}
