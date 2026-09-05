//! Picking and emitting the actual `invoke*` for a method symbol, adapting
//! what comes back from an erased signature, and the standard-library
//! knowledge that goes with it: `List` core members, ranges, and the
//! `is_stdlib_*` owner predicates the call paths test against.

use crate::code::Assembler;
use crate::gen::*;
use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_typer::{SeqPayload, SymKind, SymbolTable};
use std::collections::HashSet;

pub(crate) fn invoke_method(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    id: SymbolId,
    result_ty: Option<&Type>,
) {
    let s = ctx.st.get(id);
    let owner_id = s.owner;
    let mut owner = class_internal(ctx.st, owner_id);
    let owner_is_package = ctx.st.get(owner_id).kind == SymKind::Package;
    if owner_is_package {
        // `scala.math.{abs,max,min,Pi}` and `scala.reflect.runtime.universe`
        // are members of a *package object*, and the typer folds those into
        // the package symbol so `scala.math.abs` resolves at all (a package
        // has no runtime value of its own). scalac emits a static forwarder
        // for each on `<pkg>/package`, which is the ABI to call.
        owner = format!("{owner}/package");
    }
    let name = s.name.as_str();
    let mut desc = method_desc_boxed(ctx.st, id, ctx.boxed_vars);
    if name == "<init>" {
        // `this(...)` in an auxiliary constructor: the target takes `$outer`
        // first, and `gen_apply` has already pushed it.
        let d = with_enclosing_outer_param(ctx.st, owner_id, &desc);
        asm.invokespecial(&owner, "<init>", &d);
        return;
    }
    if owner_is_package && ctx.library_abi {
        asm.invokestatic(&owner, name, &desc);
        maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
        return;
    }
    if s.flags.contains(Flags::STATIC) {
        // JVMS 4.4.2: a method declared by an *interface* -- `static` ones
        // included -- must be named by a `CONSTANT_InterfaceMethodref`, never
        // a `CONSTANT_Methodref`. The instruction is still `invokestatic`;
        // only the constant's tag differs. Emitting a plain `Methodref` type
        // checked fine and then died at the first call with
        // `IncompatibleClassChangeError: Method '…java.util.Map.entry(…)'
        // must be InterfaceMethodref constant` -- a silent miscompile of
        // every Java 9+ interface factory (`Map.entry`, `List.of`, `Set.of`,
        // `Comparator.comparing`, …).
        if is_interface_sym(ctx.st, owner_id) && !is_module_class(ctx.st, owner_id) {
            asm.invokestatic_interface(&owner, name, &desc);
        } else {
            asm.invokestatic(&owner, name, &desc);
        }
        maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
        return;
    }
    if ctx.library_abi && !pickled_with_implicit_clause(ctx.st, id) {
        // `MapOps.map` / `flatMap` / `collect` *build a map*: they require the
        // function to return a pair, and 2.13 picks the `IterableOps` overload
        // of the same name whenever it does not
        // (`m.map { case (_, v) => v.sum }` is an `Iterable[Int]`). scala-rs
        // has one symbol for the pair, so the call has to follow the static
        // result type -- calling `MapOps.map` with an `Int`-returning function
        // threw `ClassCastException: Integer cannot be cast to Tuple2`.
        if matches!(name, "map" | "flatMap" | "collect")
            && desc.ends_with(")Lscala/collection/IterableOps;")
            && !result_ty.is_some_and(|t| builds_pairs(ctx, t))
        {
            let d = if name == "collect" {
                "(Lscala/PartialFunction;)Ljava/lang/Object;"
            } else {
                "(Lscala/Function1;)Ljava/lang/Object;"
            };
            asm.invokeinterface("scala/collection/IterableOps", name, d);
            maybe_unbox_erased_result(asm, ctx, d, result_ty);
            return;
        }
        if owner == "scala/reflect/ClassTag$" {
            let desc = match name {
                "Byte" => "()Lscala/reflect/ManifestFactory$ByteManifest;",
                "Short" => "()Lscala/reflect/ManifestFactory$ShortManifest;",
                "Char" => "()Lscala/reflect/ManifestFactory$CharManifest;",
                "Int" => "()Lscala/reflect/ManifestFactory$IntManifest;",
                "Long" => "()Lscala/reflect/ManifestFactory$LongManifest;",
                "Float" => "()Lscala/reflect/ManifestFactory$FloatManifest;",
                "Double" => "()Lscala/reflect/ManifestFactory$DoubleManifest;",
                "Boolean" => "()Lscala/reflect/ManifestFactory$BooleanManifest;",
                "Unit" => "()Lscala/reflect/ManifestFactory$UnitManifest;",
                "Any" | "AnyRef" | "AnyVal" | "Object" | "Nothing" | "Null" => {
                    "()Lscala/reflect/ClassTag;"
                }
                _ => desc.as_str(),
            };
            asm.invokevirtual(&owner, name, desc);
            return;
        }
        if name == "newArray" && owner == "scala/reflect/ClassTag" {
            asm.invokeinterface(&owner, "newArray", "(I)Ljava/lang/Object;");
            if let Some(ty) = result_ty {
                if let Type::Array(elem) = ty {
                    if is_concrete_array_elem(elem) {
                        asm.checkcast(&jvm_desc(ctx.st, ty));
                    }
                }
            }
            return;
        }
        if owner == "scala/Array$" && name == "apply" {
            // `scala.Array` declares *ten* `apply`s: the generic
            // `apply[T](xs: T*)(implicit ClassTag[T]): Array[T]`, which is the
            // one the prelude writes by hand, and nine monomorphic ones
            // (`apply(x: Int, xs: Int*): Array[Int]`, one per primitive plus
            // `Unit`) that only exist once `PickleSupply` has installed them.
            // Whether the typer can see them depends on what ran earlier in
            // the same compilation: an explicit `Array[T](…)` anywhere in the
            // file installs the set (`widen_module_from_pickle`), and from
            // then on `Array(3, 1, 2)` resolves to the `Int` overload.
            // Hard-coding the generic descriptor for all ten therefore pushed
            // an `int` where a `Seq` was declared -- `Array(3, 1, 2)` after an
            // `Array[Any](1, "a")` in the same file was a `VerifyError`, while
            // the same line on its own was fine. The monomorphic ones have no
            // type parameters and a descriptor of their own, which is already
            // the erasure of what the typer picked.
            if ctx.st.get(id).tparams.is_empty() {
                asm.invokevirtual("scala/Array$", "apply", &desc);
                return;
            }
            asm.invokevirtual(
                "scala/Array$",
                "apply",
                "(Lscala/collection/immutable/Seq;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            );
            if let Some(ty) = result_ty {
                if let Type::Array(elem) = ty {
                    if is_concrete_array_elem(elem) {
                        asm.checkcast(&jvm_desc(ctx.st, ty));
                    }
                }
            }
            return;
        }
        if owner == "scala/util/matching/Regex" {
            match name {
                "findFirstIn" => {
                    asm.invokevirtual(
                        "scala/util/matching/Regex",
                        "findFirstIn",
                        "(Ljava/lang/CharSequence;)Lscala/Option;",
                    );
                    return;
                }
                "matches" => {
                    asm.invokevirtual(
                        "scala/util/matching/Regex",
                        "matches",
                        "(Ljava/lang/CharSequence;)Z",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_bitset(&owner) {
            match name {
                "contains" => {
                    asm.invokevirtual("scala/collection/immutable/BitSet", "contains", "(I)Z");
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/BitSet",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_list(&owner) && emit_list_core_member(asm, ctx, name, id, result_ty) {
            return;
        }
        if owner == "scala/collection/Iterator" && name == "toList" {
            // `Iterator.toList` is a default method on `IterableOnceOps`.
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                "toList",
                "()Lscala/collection/immutable/List;",
            );
            return;
        }
        if name == "view" && is_stdlib_list(&owner) {
            asm.invokeinterface(
                "scala/collection/SeqOps",
                "view",
                "()Lscala/collection/SeqView;",
            );
            return;
        }
        if name == "withFilter" && is_stdlib_list(&owner) {
            asm.invokeinterface(
                "scala/collection/IterableOps",
                "withFilter",
                "(Lscala/Function1;)Lscala/collection/WithFilter;",
            );
            return;
        }
        if name == "collect" && is_stdlib_list(&owner) {
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "collect",
                "(Lscala/PartialFunction;)Lscala/collection/immutable/List;",
            );
            return;
        }
        if name == "withFilter" && is_stdlib_option(&owner) {
            asm.invokevirtual(
                "scala/Option",
                "withFilter",
                "(Lscala/Function1;)Lscala/Option$WithFilter;",
            );
            return;
        }
        // nsc: `def flatten[B](implicit ev: A <:< Option[B]): Option[B]`. The
        // evidence is erased, so `<:<.refl` (an `=:=`, hence a `<:<`) is the
        // witness scalac itself would summon.
        if name == "flatten" && is_stdlib_option(&owner) {
            asm.getstatic(
                "scala/$less$colon$less$",
                "MODULE$",
                "Lscala/$less$colon$less$;",
            );
            asm.invokevirtual("scala/$less$colon$less$", "refl", "()Lscala/$eq$colon$eq;");
            asm.invokevirtual(
                "scala/Option",
                "flatten",
                "(Lscala/$less$colon$less;)Lscala/Option;",
            );
            return;
        }
        if owner == "scala/collection/WithFilter" && (name == "map" || name == "flatMap") {
            asm.invokevirtual(&owner, name, "(Lscala/Function1;)Ljava/lang/Object;");
            if let Some(ty) = result_ty {
                let cls = jvm_desc(ctx.st, ty);
                if let Some(inner) = cls.strip_prefix('L').and_then(|s| s.strip_suffix(';')) {
                    asm.checkcast(inner);
                }
            }
            return;
        }
        if name == "tail" && is_stdlib_list(&owner) {
            if is_interface_sym(ctx.st, owner_id) {
                asm.invokeinterface(&owner, "tail", "()Lscala/collection/LinearSeq;");
            } else {
                asm.invokevirtual(&owner, "tail", "()Lscala/collection/LinearSeq;");
            }
            asm.checkcast("scala/collection/immutable/List");
            return;
        } else if name == "unapplySeq" && is_list_module_owner(&owner) {
            desc = "(Lscala/collection/SeqOps;)Lscala/collection/SeqOps;".into();
        } else if name == "unapplySeq" && SEQPAT_SEQOPS_MODULES.contains(&owner.as_str()) {
            // `SeqFactory.unapplySeq` is identity; the mixin forwarder on each
            // companion has the `SeqOps` descriptor, not the prelude `Option`.
            desc = "(Lscala/collection/SeqOps;)Lscala/collection/SeqOps;".into();
        } else if name == "unapplySeq" && owner == SEQPAT_ARRAY_MODULE {
            desc = "(Ljava/lang/Object;)Ljava/lang/Object;".into();
        }
        if is_stdlib_map_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Map$",
                        "empty",
                        "()Lscala/collection/immutable/Map;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Map$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Map");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_vector_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector$",
                        "empty",
                        "()Lscala/collection/immutable/Vector;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Vector");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_indexedseq_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/IndexedSeq$",
                        "empty",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/IndexedSeq$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_queue_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue$",
                        "empty",
                        "()Lscala/collection/immutable/Queue;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Queue");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraybuffer_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer$",
                        "empty",
                        "()Lscala/collection/mutable/ArrayBuffer;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/ArrayBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_map_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Map$",
                        "empty",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Map");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Map$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Map");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_set_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Set$",
                        "empty",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Set");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Set$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Set");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraydeque_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque$",
                        "empty",
                        "()Lscala/collection/mutable/ArrayDeque;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/ArrayDeque");
                    return;
                }
                _ => {}
            }
        }
        // The `scala.collection.mutable` factories `prelude_mutcoll` declares.
        // Every one of these inherits `apply` from `IterableFactory` /
        // `SortedIterableFactory` / `EvidenceIterableFactory`, so the JVM
        // method takes an `immutable.Seq` (plus the evidence, erased to
        // `Object` except on `TreeMap$`) and returns `Object`; `empty` is
        // overridden per companion and returns the collection itself.
        if let Some((cls, apply_desc, empty_desc)) = stdlib_mutcoll_factory(&owner) {
            let module_cls = format!("{cls}$");
            match name {
                "apply" => {
                    asm.invokevirtual(&module_cls, "apply", apply_desc);
                    asm.checkcast(cls);
                    return;
                }
                "empty" => {
                    asm.invokevirtual(&module_cls, "empty", empty_desc);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_listbuffer_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer$",
                        "empty",
                        "()Lscala/collection/mutable/ListBuffer;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/ListBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashmap_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap$",
                        "empty",
                        "()Lscala/collection/mutable/HashMap;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/HashMap");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashmap_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap$",
                        "empty",
                        "()Lscala/collection/mutable/LinkedHashMap;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/LinkedHashMap");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashset_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet$",
                        "empty",
                        "()Lscala/collection/mutable/HashSet;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/HashSet");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashset_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet$",
                        "empty",
                        "()Lscala/collection/mutable/LinkedHashSet;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/LinkedHashSet");
                    return;
                }
                _ => {}
            }
        }
        if is_list_module_owner(&owner) && name == "apply" {
            asm.invokevirtual(
                "scala/collection/immutable/List$",
                "apply",
                "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
            );
            asm.checkcast("scala/collection/immutable/List");
            return;
        }
        if is_stdlib_sortedset_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/SortedSet$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/SortedSet");
                return;
            }
        }
        if is_stdlib_treeset_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/TreeSet$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/TreeSet");
                return;
            }
        }
        if is_stdlib_sortedmap_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/SortedMap$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Lscala/math/Ordering;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/SortedMap");
                return;
            }
        }
        if is_stdlib_treemap_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/TreeMap$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Lscala/math/Ordering;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/TreeMap");
                return;
            }
        }
        if is_stdlib_bitset_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/BitSet$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/BitSet");
                return;
            }
        }
        if is_stdlib_set_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Set$",
                        "empty",
                        "()Lscala/collection/immutable/Set;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Set$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_set(&owner) {
            match name {
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/SetOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "+" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SetOps",
                        "+",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/SetOps;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Set");
                    return;
                }
                "-" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SetOps",
                        "-",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/SetOps;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Set");
                    return;
                }
                "++" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "++",
                        "(Lscala/collection/IterableOnce;)Ljava/lang/Object;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Set");
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Set");
                    return;
                }
                "map" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "map",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Set");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "head" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "head",
                        "()Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else {
                            checkcast_to(asm, ctx, result_ty, "java/lang/Object");
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_seq_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Seq$",
                        "empty",
                        "()Lscala/collection/SeqOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Seq");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Seq$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Seq");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_iterable_module(&owner) && name == "apply" {
            // `Iterable$.apply` is inherited from `IterableFactory$Delegate`
            // (not declared directly on `Iterable$`); its erased JVM
            // descriptor returns `Object`, exactly like `Seq$.apply` above.
            // See `crates/typer/src/prelude_sgap.rs::add_iterable_apply`.
            asm.invokevirtual(
                "scala/collection/Iterable$",
                "apply",
                "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
            );
            asm.checkcast("scala/collection/Iterable");
            return;
        }
        if is_stdlib_lazylist_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList$",
                        "empty",
                        "()Lscala/collection/immutable/LazyList;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/LazyList");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_seq(&owner) {
            match name {
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        } else if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_lazylist(&owner) {
            match name {
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_either(&owner) {
            match name {
                "isLeft" => {
                    asm.invokevirtual("scala/util/Either", "isLeft", "()Z");
                    return;
                }
                "getOrElse" => {
                    asm.invokevirtual(
                        "scala/util/Either",
                        "getOrElse",
                        "(Lscala/Function0;)Ljava/lang/Object;",
                    );
                    // `getOrElse[B1 >: B]` erases its result to `Object`, so
                    // anything the typer knows about it -- since
                    // `prelude_dbio` gave the method its real signature, that
                    // is now a class, not `Any` -- needs the narrowing the
                    // JVM verifier demands. `lazy_cell_from_object` is the
                    // existing "bring an `Object` back to `ret`" helper; it
                    // unboxes a primitive and no-ops when `ret` erases to
                    // `Object` anyway.
                    if let Some(ty) = result_ty {
                        lazy_cell_from_object(asm, ctx, ty);
                    }
                    return;
                }
                "map" => {
                    asm.invokevirtual(
                        "scala/util/Either",
                        "map",
                        "(Lscala/Function1;)Lscala/util/Either;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_either_module(&owner) && name == "apply" {
            let cls = if owner.ends_with("Left$") {
                "scala/util/Left"
            } else {
                "scala/util/Right"
            };
            asm.invokevirtual(&owner, "apply", &format!("(Ljava/lang/Object;)L{cls};"));
            return;
        }
        if is_stdlib_try(&owner) {
            match name {
                "getOrElse" => {
                    asm.invokevirtual(
                        "scala/util/Try",
                        "getOrElse",
                        "(Lscala/Function0;)Ljava/lang/Object;",
                    );
                    // See the `Either.getOrElse` case above: `[U >: T]` erases
                    // to `Object`, and the verifier wants the narrowing.
                    if let Some(ty) = result_ty {
                        lazy_cell_from_object(asm, ctx, ty);
                    }
                    return;
                }
                "map" => {
                    asm.invokevirtual(
                        "scala/util/Try",
                        "map",
                        "(Lscala/Function1;)Lscala/util/Try;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_breaks(&owner) {
            match name {
                "breakable" => {
                    asm.invokevirtual(&owner, "breakable", "(Lscala/Function0;)V");
                    return;
                }
                "break" => {
                    asm.invokevirtual(&owner, "break", "()Lscala/runtime/Nothing$;");
                    // nsc 2.13.16: `break()` returns Nothing$ and is followed by athrow.
                    asm.athrow();
                    return;
                }
                "tryBreakable" => {
                    asm.invokevirtual(
                        &owner,
                        "tryBreakable",
                        "(Lscala/Function0;)Lscala/util/control/Breaks$TryBlock;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if owner == "scala/util/control/Breaks$TryBlock" && name == "catchBreak" {
            asm.invokeinterface(
                "scala/util/control/Breaks$TryBlock",
                "catchBreak",
                "(Lscala/Function0;)Ljava/lang/Object;",
            );
            if let Some(ty) = result_ty {
                if is_unit_like(ty) {
                    // JVM returns boxed Unit; a statement must pop it.
                    asm.pop();
                } else {
                    maybe_unbox_erased_result(
                        asm,
                        ctx,
                        "(Lscala/Function0;)Ljava/lang/Object;",
                        result_ty,
                    );
                }
            }
            return;
        }
        if owner == "scala/util/Using$" && name == "resource" {
            let desc = "(Ljava/lang/Object;Lscala/Function1;Lscala/util/Using$Releasable;)Ljava/lang/Object;";
            asm.invokevirtual("scala/util/Using$", "resource", desc);
            if result_ty.is_some_and(is_unit_like) {
                asm.pop();
            } else {
                maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            }
            return;
        }
        if owner == "scala/util/Using$" && name == "apply" {
            asm.invokevirtual(
                "scala/util/Using$",
                "apply",
                "(Lscala/Function0;Lscala/Function1;Lscala/util/Using$Releasable;)Lscala/util/Try;",
            );
            return;
        }
        if owner == "scala/util/Using$" && name == "resources" {
            let desc = if s.jvm_name.starts_with('(') {
                s.jvm_name.clone()
            } else {
                "(Ljava/lang/Object;Lscala/Function0;Lscala/Function2;Lscala/util/Using$Releasable;Lscala/util/Using$Releasable;)Ljava/lang/Object;".into()
            };
            asm.invokevirtual("scala/util/Using$", "resources", &desc);
            if result_ty.is_some_and(is_unit_like) {
                asm.pop();
            } else {
                maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
            }
            return;
        }
        if owner == "scala/util/Using$Manager$" && name == "apply" {
            asm.invokevirtual(
                "scala/util/Using$Manager$",
                "apply",
                "(Lscala/Function1;)Lscala/util/Try;",
            );
            return;
        }
        if owner == "scala/util/Using$Manager" {
            match name {
                "apply" => {
                    let desc =
                        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)Ljava/lang/Object;";
                    asm.invokevirtual("scala/util/Using$Manager", "apply", desc);
                    if result_ty.is_some_and(is_unit_like) {
                        asm.pop();
                    } else {
                        maybe_unbox_erased_result(asm, ctx, desc, result_ty);
                    }
                    return;
                }
                "acquire" => {
                    asm.invokevirtual(
                        "scala/util/Using$Manager",
                        "acquire",
                        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_try_module(&owner) && name == "apply" {
            match owner.as_str() {
                "scala/util/Try$" => {
                    asm.invokevirtual(
                        "scala/util/Try$",
                        "apply",
                        "(Lscala/Function0;)Lscala/util/Try;",
                    );
                    return;
                }
                "scala/util/Success$" => {
                    asm.invokevirtual(
                        "scala/util/Success$",
                        "apply",
                        "(Ljava/lang/Object;)Lscala/util/Success;",
                    );
                    return;
                }
                "scala/util/Failure$" => {
                    asm.invokevirtual(
                        "scala/util/Failure$",
                        "apply",
                        "(Ljava/lang/Throwable;)Lscala/util/Failure;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_sortedmap(&owner) {
            match name {
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SortedMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SortedMap",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SortedMap",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_treemap(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/TreeMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/TreeMap",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/TreeMap",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_map(&owner) {
            match name {
                "updated" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "updated",
                        "(Ljava/lang/Object;Ljava/lang/Object;)Lscala/collection/immutable/MapOps;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Map");
                    return;
                }
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "+" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "$plus",
                        "(Lscala/Tuple2;)Lscala/collection/immutable/MapOps;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Map");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "getOrElse" => {
                    let d = "(Ljava/lang/Object;Lscala/Function0;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/MapOps", "getOrElse", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "keys" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "keys",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "values" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "values",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "keySet" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "keySet",
                        "()Lscala/collection/immutable/Set;",
                    );
                    return;
                }
                "-" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "-",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/MapOps;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Map");
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/immutable/Map");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "head" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "head",
                        "()Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/Tuple2");
                    return;
                }
                "foldLeft" => {
                    let d = "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/IterableOnceOps", "foldLeft", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                "withDefaultValue" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/Map",
                        "withDefaultValue",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/Map;",
                    );
                    return;
                }
                "view" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "view",
                        "()Lscala/collection/MapView;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mapview(&owner) {
            match name {
                "keys" => {
                    asm.invokeinterface(
                        "scala/collection/MapView",
                        "keys",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "values" => {
                    asm.invokeinterface(
                        "scala/collection/MapView",
                        "values",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "filterKeys" => {
                    asm.invokeinterface(
                        "scala/collection/MapView",
                        "filterKeys",
                        "(Lscala/Function1;)Lscala/collection/MapView;",
                    );
                    return;
                }
                "mapValues" => {
                    asm.invokeinterface(
                        "scala/collection/MapView",
                        "mapValues",
                        "(Lscala/Function1;)Lscala/collection/MapView;",
                    );
                    return;
                }
                "toMap" => {
                    // `IterableOnceOps.toMap` needs an `A <:< (K, V)` witness;
                    // a `MapView`'s element type is exactly `(K, V)`, so the
                    // reflexive `scala.$less$colon$less$.MODULE$.refl()` applies.
                    asm.getstatic(
                        "scala/$less$colon$less$",
                        "MODULE$",
                        "Lscala/$less$colon$less$;",
                    );
                    asm.invokevirtual("scala/$less$colon$less$", "refl", "()Lscala/$eq$colon$eq;");
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toMap",
                        "(Lscala/$less$colon$less;)Lscala/collection/immutable/Map;",
                    );
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_vector(&owner) {
            match name {
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                ":+" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "$colon$plus",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Vector");
                    return;
                }
                "updated" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector",
                        "updated",
                        "(ILjava/lang/Object;)Lscala/collection/immutable/Vector;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "map" | "filter" => {
                    let desc = "(Lscala/Function1;)Ljava/lang/Object;";
                    asm.invokevirtual("scala/collection/immutable/Vector", name, desc);
                    checkcast_to(asm, ctx, result_ty, "scala/collection/immutable/Vector");
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "head" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "head",
                        "()Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else {
                            checkcast_to(asm, ctx, result_ty, "java/lang/Object");
                        }
                    }
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "foldLeft" => {
                    let d = "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/IterableOnceOps", "foldLeft", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_indexedseq(&owner) {
            if name == "apply" {
                asm.invokeinterface("scala/collection/SeqOps", "apply", "(I)Ljava/lang/Object;");
                if let Some(ty) = result_ty {
                    if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                        let cls = jvm_desc(ctx.st, ty);
                        if let Some(inner) = cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                        {
                            if inner != "java/lang/Object" {
                                asm.checkcast(inner);
                            }
                        }
                    }
                }
                return;
            }
        }
        if is_stdlib_queue(&owner) {
            match name {
                "enqueue" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue",
                        "enqueue",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/Queue;",
                    );
                    return;
                }
                "dequeue" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue",
                        "dequeue",
                        "()Lscala/Tuple2;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraybuffer(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "update" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer",
                        "update",
                        "(ILjava/lang/Object;)V",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "map" | "filter" | "reverse" => {
                    let d = "(Lscala/Function1;)Ljava/lang/Object;";
                    let d0 = "()Ljava/lang/Object;";
                    let desc = if name == "reverse" { d0 } else { d };
                    asm.invokevirtual("scala/collection/mutable/ArrayBuffer", name, desc);
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "append" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Buffer",
                        "append",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Buffer;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "++=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "++=",
                        "(Lscala/collection/IterableOnce;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "sortBy" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sortBy",
                        "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "sorted" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sorted",
                        "(Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraydeque(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "prepend" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "prepend",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/ArrayDeque;",
                    );
                    return;
                }
                // `append` is a `Buffer` default method that `ArrayDeque`
                // does not override, so it returns `Buffer`, not the
                // receiver's own class as `prepend` does.
                "append" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "append",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Buffer;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayDeque");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_listbuffer(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "map" | "filter" => {
                    let desc = "(Lscala/Function1;)Ljava/lang/Object;";
                    asm.invokevirtual("scala/collection/mutable/ListBuffer", name, desc);
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "reverse" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "reverse",
                        "()Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "append" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Buffer",
                        "append",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Buffer;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "++=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "++=",
                        "(Lscala/collection/IterableOnce;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "sortBy" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sortBy",
                        "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "sorted" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sorted",
                        "(Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_map(&owner) {
            let d_obj_obj = "(Ljava/lang/Object;)Ljava/lang/Object;";
            match name {
                "apply" => {
                    asm.invokeinterface("scala/collection/MapOps", "apply", d_obj_obj);
                    maybe_unbox_erased_result(asm, ctx, d_obj_obj, result_ty);
                    return;
                }
                "get" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "update" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/MapOps",
                        "update",
                        "(Ljava/lang/Object;Ljava/lang/Object;)V",
                    );
                    return;
                }
                // `javap -p -s scala.collection.mutable.MapOps`:
                // `public default C $minus(K)`.
                "-" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/MapOps",
                        "$minus",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/MapOps;",
                    );
                    cast_collection_result(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "keys" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "keys",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "values" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "values",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "+=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "remove" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/MapOps",
                        "remove",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "clear" => {
                    asm.invokeinterface("scala/collection/mutable/MapOps", "clear", "()V");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "getOrElse" => {
                    let d = "(Ljava/lang/Object;Lscala/Function0;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/MapOps", "getOrElse", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                "getOrElseUpdate" => {
                    let d = "(Ljava/lang/Object;Lscala/Function0;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/mutable/MapOps", "getOrElseUpdate", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_set(&owner) {
            match name {
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/SetOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "+=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "remove" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/SetOps",
                        "remove",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "clear" => {
                    asm.invokeinterface("scala/collection/mutable/Clearable", "clear", "()V");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "map" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "map",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_coll_iterable(&owner) {
            match name {
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mapview(&owner) {
            match name {
                "mapValues" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "mapValues",
                        "(Lscala/Function1;)Lscala/collection/MapView;",
                    );
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashmap(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "update" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "update",
                        "(Ljava/lang/Object;Ljava/lang/Object;)V",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashmap(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "update" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "update",
                        "(Ljava/lang/Object;Ljava/lang/Object;)V",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashset(&owner) {
            match name {
                "contains" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashset(&owner) {
            match name {
                "contains" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_stringbuilder(&owner) {
            match name {
                // `+=(Char)` has no direct override on StringBuilder (only the
                // erased `Growable.$plus$eq(Object): Growable`); `addOne(Char)`
                // is a concrete, non-erased override with the same effect.
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/StringBuilder",
                        "addOne",
                        "(C)Lscala/collection/mutable/StringBuilder;",
                    );
                    return;
                }
                // `++=(String)` similarly has no direct override; `addAll(String)`
                // is the concrete, non-erased equivalent.
                "++=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/StringBuilder",
                        "addAll",
                        "(Ljava/lang/String;)Lscala/collection/mutable/StringBuilder;",
                    );
                    return;
                }
                // `append` has a concrete overload per argument type; use the
                // descriptor computed from the resolved (overloaded) symbol.
                "append" | "insert" | "deleteCharAt" | "setLength" | "clear" | "isEmpty"
                | "nonEmpty" | "length" | "toString" | "result" | "charAt" | "apply" => {
                    asm.invokevirtual("scala/collection/mutable/StringBuilder", name, &desc);
                    return;
                }
                // `reverse` is inherited from `IndexedSeqOps` and erased to
                // `Object`; checkcast back to the declared `StringBuilder`.
                "reverse" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/StringBuilder",
                        "reverse",
                        "()Ljava/lang/Object;",
                    );
                    cast_collection_result(
                        asm,
                        ctx,
                        result_ty,
                        "scala/collection/mutable/StringBuilder",
                    );
                    return;
                }
                _ => {}
            }
        }
        // `Growable` / `Shrinkable` declare these once for every mutable
        // collection; a concrete class inherits the interface default rather
        // than overriding it, so the call is on the interface. `StringBuilder`
        // is handled above: it has its own non-erased `addAll`.
        if owner.starts_with("scala/collection/mutable/") {
            match name {
                "++=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "$plus$plus$eq",
                        "(Lscala/collection/IterableOnce;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "$minus$eq",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    return;
                }
                "--=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "$minus$minus$eq",
                        "(Lscala/collection/IterableOnce;)Lscala/collection/mutable/Shrinkable;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_range(&owner) || is_stdlib_numeric_range(&owner) {
            if name == "mkString" {
                asm.invokeinterface(
                    "scala/collection/IterableOnceOps",
                    "mkString",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                );
                return;
            }
        }
        if is_stdlib_range(&owner) {
            match name {
                // `sum`/`min`/`max` take an implicit `Numeric`/`Ordering`
                // instance; Range's own overrides return `int` directly
                // (not the generic erased `Object`), so no checkcast/unbox
                // is needed after pushing the `Int` singleton.
                "sum" => {
                    asm.getstatic(
                        "scala/math/Numeric$IntIsIntegral$",
                        "MODULE$",
                        "Lscala/math/Numeric$IntIsIntegral$;",
                    );
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        "sum",
                        "(Lscala/math/Numeric;)I",
                    );
                    return;
                }
                // `product` has no `int`-returning override on `Range`
                // itself (only the generic `IterableOnceOps` default),
                // unlike `sum`/`min`/`max`.
                "product" => {
                    asm.getstatic(
                        "scala/math/Numeric$IntIsIntegral$",
                        "MODULE$",
                        "Lscala/math/Numeric$IntIsIntegral$;",
                    );
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "product",
                        "(Lscala/math/Numeric;)Ljava/lang/Object;",
                    );
                    emit_unbox(asm, &Type::Int);
                    return;
                }
                "min" | "max" => {
                    asm.getstatic(
                        "scala/math/Ordering$Int$",
                        "MODULE$",
                        "Lscala/math/Ordering$Int$;",
                    );
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        name,
                        "(Lscala/math/Ordering;)I",
                    );
                    return;
                }
                // `filter`/`filterNot`/`flatMap`/`zipWithIndex` only have the
                // generic `Object`-erased override on `Range` (no specific
                // `IndexedSeq`-returning bridge, unlike `map`/`take`/`drop`).
                "filter" | "filterNot" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        name,
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                "flatMap" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        "flatMap",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                "zipWithIndex" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        "zipWithIndex",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                // `toArray` is only the `IterableOnceOps` generic default.
                "toArray" => {
                    asm.getstatic(
                        "scala/reflect/ClassTag$",
                        "MODULE$",
                        "Lscala/reflect/ClassTag$;",
                    );
                    asm.invokevirtual(
                        "scala/reflect/ClassTag$",
                        "Int",
                        "()Lscala/reflect/ManifestFactory$IntManifest;",
                    );
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toArray",
                        "(Lscala/reflect/ClassTag;)Ljava/lang/Object;",
                    );
                    asm.checkcast("[I");
                    return;
                }
                _ => {}
            }
        }
    }
    // A member completed from a pickle is installed on the class it was asked
    // for, but the JVM method may be declared where the receiver's class file
    // does not lead (see `Symbol::declaring_class`). Naming the receiver there
    // is a `NoSuchMethodError`.
    let declaring = &ctx.st.get(id).declaring_class;
    if !declaring.is_empty() {
        if ctx.st.get(id).declaring_is_interface {
            asm.invokeinterface(declaring, name, &desc);
        } else {
            asm.invokevirtual(declaring, name, &desc);
        }
        maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
        return;
    }
    if is_interface_sym(ctx.st, owner_id) {
        // A trait-private method has no interface signature (see
        // `is_trait_private_def`): every caller is textually inside the
        // trait, so its body is a `private static <name>$` on the interface
        // and reaching it is a same-class `invokestatic`, not
        // `invokeinterface` on a declaration that doesn't exist.
        if s.flags.contains(Flags::PRIVATE) && !widened(ctx.st, id) {
            let static_desc = trait_static_desc(&owner, &desc);
            asm.invokestatic_interface(&owner, &trait_static_name(name), &static_desc);
        } else {
            asm.invokeinterface(&owner, name, &desc);
        }
    } else {
        asm.invokevirtual(&owner, name, &desc);
    }
    maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
}

/// After loading a generic field (`Object` / type param), cast or unbox to the
/// tree's instantiated type so `name + arg._1` can `append(String)`.
pub(crate) fn maybe_cast_erased_load(asm: &mut Assembler, ctx: &EmitCtx, from: &Type, want: &Type) {
    if is_jvm_primitive(want) && !is_unit_like(want) && !is_jvm_primitive(from) {
        emit_unbox(asm, want);
        return;
    }
    if matches!(want, Type::String) && !matches!(from, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if let Some(cn) = checkcast_internal(ctx.st, want) {
        let from_desc = jvm_desc(ctx.st, from);
        if from_desc == "Ljava/lang/Object;" {
            asm.checkcast(&cn);
        }
    }
}

/// Unconditional checkcast to `result_ty`'s own JVM class (falling back to
/// `fallback` when `result_ty` erases to something without a class name,
/// e.g. a raw type param). Used after invoking a mixin default method whose
/// *declared* return type (`Buffer`, `Growable`, `Shrinkable`, the bound `C`
/// of `SeqOps`, …) is narrower than the concrete collection type we model in
/// the prelude (`ArrayBuffer[A]`, `ListBuffer[A]`, …) — unlike
/// `maybe_unbox_erased_result`, this does not require the invoked
/// descriptor's return type to literally be `Ljava/lang/Object;`.
/// Picks the right `mkString` overload descriptor for the resolved symbol
/// `id` (0/1/3 `String` params — the typer already chose the overload; we
/// just need to know which arity to emit against `IterableOnceOps`).
pub(crate) fn mkstring_desc(st: &SymbolTable, id: SymbolId) -> &'static str {
    let n = match &st.get(id).ty {
        Type::Method { paramss, .. } => paramss.first().map(|p| p.len()).unwrap_or(0),
        _ => 0,
    };
    match n {
        0 => "()Ljava/lang/String;",
        1 => "(Ljava/lang/String;)Ljava/lang/String;",
        _ => "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
    }
}

pub(crate) fn checkcast_to(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    result_ty: Option<&Type>,
    fallback: &str,
) {
    if let Some(ty) = result_ty {
        if let Some(cn) = checkcast_internal(ctx.st, ty) {
            asm.checkcast(&cn);
            return;
        }
    }
    asm.checkcast(fallback);
}

/// After a generic invoke that returns `Object`, unbox when the tree still has
/// a primitive (e.g. `Iterator.next` / `Option.get` as `Int`).
pub(crate) fn maybe_unbox_erased_result(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    desc: &str,
    result_ty: Option<&Type>,
) {
    let Some(ty) = result_ty else {
        return;
    };
    if !desc_returns_object(desc) {
        // The descriptor may still be wider than what the typer settled on:
        // `TreeMap[K, V] - key` is declared to return `Map` on the JVM, and
        // `2.13`'s own signature narrows it to `C`. Without the cast the
        // verifier sees a `Map` where a `TreeMap` is wanted.
        //
        // The old test was "does `ty` reach `declared` through its parents",
        // which only fires for an ancestor the *prelude* models. The library's
        // `…Ops` traits are deliberately left out of that hierarchy
        // (`prelude_hier`), so every `IterableFactory` member whose `CC` is
        // bounded by one — `List$.fill`/`tabulate`/`concat` are declared
        // `()Lscala/collection/SeqOps;` on the JVM, exactly as in the real jar
        // — got no cast and blew up in the verifier at the first use of the
        // result. Ask the opposite, decidable question instead: is the
        // descriptor's return type *known* to conform to what we want? If we
        // cannot show that, cast, which is what scalac's erasure does.
        if let (Some(declared), Some(want)) =
            (desc_return_internal(desc), checkcast_internal(ctx.st, ty))
        {
            if declared != want
                && has_class_sym(ctx.st, ty)
                && !internal_conforms(ctx.st, &declared, &want)
            {
                asm.checkcast(&want);
            }
        }
        // An *array of* `Object` is just as erased as a bare one:
        // `Array$.ofDim(IILscala/reflect/ClassTag;)` is declared
        // `[Ljava/lang/Object;` while `Array.ofDim[Double](2, 2)` is `[[D`.
        // scalac emits the `checkcast "[[D"`; without it the `dastore` that
        // follows sees a `java/lang/Object` and the method fails verification.
        if let (Some((declared, depth)), Type::Array(elem)) = (erased_array_return(desc), ty) {
            let want = jvm_desc(ctx.st, ty);
            if want != declared
                && want.len() - want.trim_start_matches('[').len() >= depth
                && is_concrete_array_elem(elem)
            {
                asm.checkcast(&want);
            }
        }
        return;
    }
    if is_jvm_primitive(ty) && !is_unit_like(ty) {
        // Private-runtime Option/Iterator already emit unboxed shapes; the
        // library ABI erases those to Object.
        if ctx.library_abi {
            emit_unbox(asm, ty);
        }
        return;
    }
    if matches!(ty, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if let Type::Array(elem) = ty {
        if is_concrete_array_elem(elem) {
            let d = jvm_desc(ctx.st, ty);
            asm.checkcast(&d);
        }
        return;
    }
    if let Some(cn) = checkcast_internal(ctx.st, ty) {
        asm.checkcast(&cn);
    }
}

/// Lambda captures (and similar) are stored as `Object`. Restore the JVM type
/// before the body uses the local (`iastore` needs `[I`, not `Object`).
pub(crate) fn emit_from_erased_object(asm: &mut Assembler, st: &SymbolTable, ty: &Type) {
    let ty = &ty.widen_constant();
    if is_jvm_primitive(ty) {
        emit_unbox(asm, ty);
        return;
    }
    if matches!(ty, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if matches!(ty, Type::Array(_)) {
        asm.checkcast(&jvm_desc(st, ty));
        return;
    }
    if let Type::Class { sym, .. } = ty {
        let n = class_internal(st, *sym);
        if !n.is_empty() && n != "java/lang/Object" {
            asm.checkcast(&n);
        }
        return;
    }
    if matches!(ty, Type::Tuple(_)) {
        asm.checkcast("scala/Tuple2");
    }
}

/// The internal name a descriptor returns, for an ordinary reference return
/// (`(…)Lscala/collection/immutable/Map;` -> `scala/collection/immutable/Map`).
/// `None` for primitives, arrays and `void`.
pub(crate) fn desc_return_internal(desc: &str) -> Option<String> {
    let (_, ret) = desc.rsplit_once(')')?;
    let inner = ret.strip_prefix('L')?.strip_suffix(';')?;
    (!inner.is_empty()).then(|| inner.to_string())
}

/// Does `ty` erase to a class we actually know, as opposed to a bare type
/// parameter or an unresolved name? `checkcast_internal` happily turns
/// `Type::Named { name: "A" }` into `A`, and casting to that would be a
/// `NoClassDefFoundError`.
pub(crate) fn has_class_sym(st: &SymbolTable, ty: &Type) -> bool {
    matches!(ty, Type::String | Type::Function { .. } | Type::Tuple(_))
        || st.class_sym_of(ty).is_some()
}

/// The class-like symbol whose JVM internal name is `internal`.
pub(crate) fn class_sym_named(st: &SymbolTable, internal: &str) -> Option<SymbolId> {
    st.symbols
        .iter()
        .find(|s| {
            s.is_class_like()
                && (s.jvm_name == internal
                    // Source classes in the default package carry no
                    // `jvm_name`; their internal name is built from the owners.
                    || (s.jvm_name.is_empty() && class_internal(st, s.id) == internal))
        })
        .map(|s| s.id)
}

/// Is the JVM class `from` *known* to conform to `to`?
///
/// Deliberately one-sided: a `false` means "we cannot show it", not "it does
/// not". Callers use it to decide whether a `checkcast` is needed, and an
/// unnecessary one only costs three bytes, while a missing one costs the whole
/// method to `VerifyError`. The library's `…Ops` traits are exactly the case
/// that has to come out `false`: `scala/collection/SeqOps` is not in the
/// prelude's hierarchy, so nothing here can show that a `List` is one.
pub(crate) fn internal_conforms(st: &SymbolTable, from: &str, to: &str) -> bool {
    if from == to || to == "java/lang/Object" {
        return true;
    }
    let Some(start) = class_sym_named(st, from) else {
        return false;
    };
    let mut work = vec![start];
    let mut seen = HashSet::new();
    while let Some(id) = work.pop() {
        if !seen.insert(id.0) {
            continue;
        }
        if class_internal(st, id) == to {
            return true;
        }
        for p in st.get(id).parents.clone() {
            let p = st.function_class_form(&p).unwrap_or(p);
            if let Some(pid) = st.class_sym_of(&p) {
                work.push(pid);
            }
        }
    }
    false
}

/// An erased array-of-reference return: the descriptor and its `[` depth, for
/// `(…)[Ljava/lang/Object;` and deeper. A generic signature may narrow such a
/// return to a wholly different array descriptor.
pub(crate) fn erased_array_return(desc: &str) -> Option<(String, usize)> {
    let (_, ret) = desc.rsplit_once(')')?;
    let elem = ret.trim_start_matches('[');
    let depth = ret.len() - elem.len();
    (depth > 0 && elem == "Ljava/lang/Object;").then(|| (ret.to_string(), depth))
}

/// The cast a stdlib collection call's result needs.
///
/// These dispatch arms hardcode the descriptor they emit, so they also have to
/// hardcode the cast that follows. The typer narrows a member declared to
/// return `C` to the receiver's own collection (`TreeMap - key` is a
/// `TreeMap`, not a `Map`), so casting to the *declared* class unconditionally
/// left a `Map` on the stack where a `TreeMap` was wanted -- `VerifyError: Bad
/// type on operand stack`. Take the typer's own result type whenever it is a
/// subclass of `fallback`, and `fallback` otherwise.
pub(crate) fn cast_collection_result(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    result_ty: Option<&Type>,
    fallback: &str,
) {
    if let Some(ty) = result_ty {
        if let Some(want) = checkcast_internal(ctx.st, ty) {
            // `internal_conforms` is one-sided: narrow to the typer's own
            // result only when it is *provably* the declared class or below.
            if want != "java/lang/Object" && internal_conforms(ctx.st, &want, fallback) {
                asm.checkcast(&want);
                return;
            }
        }
    }
    asm.checkcast(fallback);
}

/// Does this result type still hold key/value pairs? `MapOps.map` builds a
/// map and needs the function to return one; `IterableOps.map` is the
/// overload for everything else. A sorted map's `map` lands here with
/// `Iterable[(K2, V2)]` — still pairs, still `MapOps.map`.
pub(crate) fn builds_pairs(ctx: &EmitCtx, ty: &Type) -> bool {
    if checkcast_internal(ctx.st, ty)
        .is_some_and(|n| internal_conforms(ctx.st, &n, "scala/collection/Map"))
    {
        return true;
    }
    let elem = match ty {
        Type::Class { args, .. } if args.len() == 1 => args[0].clone(),
        _ => return false,
    };
    matches!(&elem, Type::Tuple(ts) if ts.len() == 2)
        || matches!(&elem, Type::Class { sym, args }
            if args.len() == 2 && ctx.st.get(*sym).name == "Tuple2")
}

pub(crate) fn desc_returns_object(desc: &str) -> bool {
    desc.rsplit_once(')')
        .map(|(_, ret)| ret == "Ljava/lang/Object;")
        .unwrap_or(false)
}

pub(crate) fn array_elem_ty(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array(t) => Some((**t).clone()),
        Type::Named { name, args } if name == "Array" && args.len() == 1 => Some(args[0].clone()),
        _ => None,
    }
}

pub(crate) fn is_concrete_array_elem(elem: &Type) -> bool {
    matches!(
        elem,
        Type::Boolean
            | Type::Int
            | Type::Long
            | Type::Double
            | Type::Float
            | Type::Char
            | Type::Byte
            | Type::Short
            | Type::String
            | Type::Class { .. }
            | Type::ModuleRef(_)
            // `Array[AnyRef]` erases to `[Ljava/lang/Object;`, not to
            // `Object`: only an *abstract* element collapses the array away,
            // so these still need the cast off `Array$.apply`'s `Object`.
            | Type::Any
            | Type::AnyRef
            | Type::AnyVal
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Function { .. }
            // `Array[Unit]` is `[Lscala/runtime/BoxedUnit;` — a concrete
            // element like any other, not the `V` that `Unit` is as a result.
            | Type::Unit
    )
}

pub(crate) fn emit_newarray(asm: &mut Assembler, ctx: &EmitCtx, elem: &Type) {
    match elem {
        Type::Boolean => asm.newarray(4),
        Type::Char => asm.newarray(5),
        Type::Float => asm.newarray(6),
        Type::Double => asm.newarray(7),
        Type::Byte => asm.newarray(8),
        Type::Short => asm.newarray(9),
        Type::Int => asm.newarray(10),
        Type::Long => asm.newarray(11),
        Type::String => asm.anewarray("java/lang/String"),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
            asm.anewarray(&class_internal(ctx.st, *sym));
        }
        // Everything else that erases to a reference: `anewarray`'s operand is
        // an *internal name*, which for an array component is the descriptor
        // itself. scalac emits `anewarray "[I"` for `new Array[Array[Int]](n)`;
        // falling through to `java/lang/Object` produced `[Ljava/lang/Object;`,
        // and the first `arr(i)(j)` then failed verification with
        // `Bad type on operand stack in iaload`. The same descriptor gives
        // `Array[(Int, Int)]` its `[Lscala/Tuple2;` and `Array[Int => Int]`
        // its `[Lscala/Function1;`, both of which scalac also emits.
        _ => {
            let desc = jvm_desc(ctx.st, elem);
            if desc.starts_with('[') {
                asm.anewarray(&desc);
            } else if let Some(inner) = desc.strip_prefix('L').and_then(|d| d.strip_suffix(';')) {
                asm.anewarray(inner);
            } else {
                // `V`: `Unit` / `Nothing`, which have no element class here.
                asm.anewarray("java/lang/Object");
            }
        }
    }
}

/// `List`'s JVM owner (scala-library 2.13.16).
pub(crate) const LIST_CLS: &str = "scala/collection/immutable/List";
pub(crate) const ITERABLE_ONCE_OPS: &str = "scala/collection/IterableOnceOps";
pub(crate) const ITERABLE_OPS: &str = "scala/collection/IterableOps";
pub(crate) const SEQ_OPS: &str = "scala/collection/SeqOps";

/// Post-processing after the invoke.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ListPost {
    /// Leave it as it is.
    None,
    /// Cast an erased result back to `List`.
    CastList,
    /// unbox / checkcast an `Object` result to match the result type.
    Erased,
}

/// Invokes for the core `List` members `prelude_seq.rs` added.
///
/// The descriptors are the ones confirmed with
/// `javap -s -cp scala-library-2.13.16.jar`. The members `List` does not have itself
/// are default methods on `IterableOnceOps` / `IterableOps` / `SeqOps`, so they are
/// called with invokeinterface.
///
/// Returns `false` for names it does not handle, leaving the caller's default invoke to it.
pub(crate) fn emit_list_core_member(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    name: &str,
    id: SymbolId,
    result_ty: Option<&Type>,
) -> bool {
    let s = ctx.st.get(id);
    let nargs = match &s.ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().count(),
        _ => s.params.len(),
    };
    // (invokeinterface?, owner, JVM name, descriptor, post-processing)
    let (iface, owner, jvm, desc, post): (bool, &str, &str, &str, ListPost) = match (name, nargs) {
        // --- virtuals on `List` itself (the result is a `List` too)
        ("map", 1) | ("flatMap", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("::", 1) => (
            false,
            LIST_CLS,
            "::",
            "(Ljava/lang/Object;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        // `indexWhere(p)` is `indexWhere(p, 0)` (a default argument).
        ("indexWhere", 1) => {
            asm.iconst(0);
            (
                false,
                LIST_CLS,
                "indexWhere",
                "(Lscala/Function1;I)I",
                ListPost::None,
            )
        }
        ("filter", 1) | ("filterNot", 1) | ("takeWhile", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("take", 1) | ("takeRight", 1) => (
            false,
            LIST_CLS,
            name,
            "(I)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("slice", 2) => (
            false,
            LIST_CLS,
            "slice",
            "(II)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("reverse", 0) | ("toList", 0) => (
            false,
            LIST_CLS,
            name,
            "()Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("updated", 2) => (
            false,
            LIST_CLS,
            "updated",
            "(ILjava/lang/Object;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("splitAt", 1) => (
            false,
            LIST_CLS,
            "splitAt",
            "(I)Lscala/Tuple2;",
            ListPost::None,
        ),
        ("span", 1) | ("partition", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Lscala/Tuple2;",
            ListPost::None,
        ),
        ("forall", 1) | ("exists", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Z",
            ListPost::None,
        ),
        ("contains", 1) => (
            false,
            LIST_CLS,
            "contains",
            "(Ljava/lang/Object;)Z",
            ListPost::None,
        ),
        ("find", 1) => (
            false,
            LIST_CLS,
            "find",
            "(Lscala/Function1;)Lscala/Option;",
            ListPost::None,
        ),
        ("headOption", 0) => (
            false,
            LIST_CLS,
            "headOption",
            "()Lscala/Option;",
            ListPost::None,
        ),
        ("last", 0) => (
            false,
            LIST_CLS,
            "last",
            "()Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("foldLeft", 2) | ("foldRight", 2) => (
            false,
            LIST_CLS,
            name,
            "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        // --- virtuals on `List`, but with an erased result
        ("drop", 1) => (
            false,
            LIST_CLS,
            "drop",
            "(I)Lscala/collection/LinearSeq;",
            ListPost::CastList,
        ),
        ("dropWhile", 1) => (
            false,
            LIST_CLS,
            "dropWhile",
            "(Lscala/Function1;)Lscala/collection/LinearSeq;",
            ListPost::CastList,
        ),
        ("dropRight", 1) => (
            false,
            LIST_CLS,
            "dropRight",
            "(I)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("distinctBy", 1) => (
            false,
            LIST_CLS,
            "distinctBy",
            "(Lscala/Function1;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("sorted", 1) => (
            false,
            LIST_CLS,
            "sorted",
            "(Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("zip", 1) => (
            false,
            LIST_CLS,
            "zip",
            "(Lscala/collection/IterableOnce;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("zipWithIndex", 0) => (
            false,
            LIST_CLS,
            "zipWithIndex",
            "()Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("scanLeft", 2) => (
            false,
            LIST_CLS,
            "scanLeft",
            "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        // --- concatenation and appending. `++` / `:++` are `appendedAll`, `++:` is `prependedAll`.
        (":::", 1) => (
            false,
            LIST_CLS,
            ":::",
            "(Lscala/collection/immutable/List;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("+:", 1) => (
            false,
            LIST_CLS,
            "prepended",
            "(Ljava/lang/Object;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        (":+", 1) => (
            false,
            LIST_CLS,
            "appended",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("++", 1) | (":++", 1) | ("concat", 1) => (
            false,
            LIST_CLS,
            "appendedAll",
            "(Lscala/collection/IterableOnce;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("++:", 1) => (
            false,
            LIST_CLS,
            "prependedAll",
            "(Lscala/collection/IterableOnce;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        // --- default methods on `IterableOnceOps`
        ("size", 0) => (true, ITERABLE_ONCE_OPS, "size", "()I", ListPost::None),
        ("nonEmpty", 0) => (true, ITERABLE_ONCE_OPS, "nonEmpty", "()Z", ListPost::None),
        ("count", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            "count",
            "(Lscala/Function1;)I",
            ListPost::None,
        ),
        ("mkString", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "mkString",
            "()Ljava/lang/String;",
            ListPost::None,
        ),
        ("mkString", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            "mkString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            ListPost::None,
        ),
        ("mkString", 3) => (
            true,
            ITERABLE_ONCE_OPS,
            "mkString",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            ListPost::None,
        ),
        ("sum", 1) | ("product", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/math/Numeric;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("min", 1) | ("max", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("minBy", 2) | ("maxBy", 2) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("reduce", 1) | ("reduceLeft", 1) | ("reduceRight", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/Function2;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("toArray", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            "toArray",
            "(Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("toSet", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "toSet",
            "()Lscala/collection/immutable/Set;",
            ListPost::None,
        ),
        ("toVector", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "toVector",
            "()Lscala/collection/immutable/Vector;",
            ListPost::None,
        ),
        ("toSeq", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "toSeq",
            "()Lscala/collection/immutable/Seq;",
            ListPost::None,
        ),
        // --- default methods on `IterableOps`
        ("init", 0) => (
            true,
            ITERABLE_OPS,
            "init",
            "()Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("lastOption", 0) => (
            true,
            ITERABLE_OPS,
            "lastOption",
            "()Lscala/Option;",
            ListPost::None,
        ),
        ("groupBy", 1) => (
            true,
            ITERABLE_OPS,
            "groupBy",
            "(Lscala/Function1;)Lscala/collection/immutable/Map;",
            ListPost::None,
        ),
        ("grouped", 1) | ("sliding", 1) => (
            true,
            ITERABLE_OPS,
            name,
            "(I)Lscala/collection/Iterator;",
            ListPost::None,
        ),
        ("sliding", 2) => (
            true,
            ITERABLE_OPS,
            "sliding",
            "(II)Lscala/collection/Iterator;",
            ListPost::None,
        ),
        // --- default methods on `SeqOps`
        ("distinct", 0) => (
            true,
            SEQ_OPS,
            "distinct",
            "()Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("sortBy", 2) => (
            true,
            SEQ_OPS,
            "sortBy",
            "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("sortWith", 1) => (
            true,
            SEQ_OPS,
            "sortWith",
            "(Lscala/Function2;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("indexOf", 1) => (
            true,
            SEQ_OPS,
            "indexOf",
            "(Ljava/lang/Object;)I",
            ListPost::None,
        ),
        ("endsWith", 1) => (
            true,
            SEQ_OPS,
            "endsWith",
            "(Lscala/collection/Iterable;)Z",
            ListPost::None,
        ),
        // `startsWith(that)` is `startsWith(that, 0)` (a default argument).
        ("startsWith", 1) => {
            asm.iconst(0);
            (
                true,
                SEQ_OPS,
                "startsWith",
                "(Lscala/collection/IterableOnce;I)Z",
                ListPost::None,
            )
        }
        _ => return false,
    };
    if iface {
        asm.invokeinterface(owner, jvm, desc);
    } else {
        asm.invokevirtual(owner, jvm, desc);
    }
    match post {
        ListPost::None => {}
        ListPost::CastList => asm.checkcast(LIST_CLS),
        ListPost::Erased => maybe_unbox_erased_result(asm, ctx, desc, result_ty),
    }
    true
}

pub(crate) fn is_stdlib_list(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/List"
            | "scala/collection/immutable/$colon$colon"
            | "scala/collection/immutable/Nil$"
    )
}

pub(crate) fn is_stdlib_option(owner: &str) -> bool {
    matches!(owner, "scala/Option" | "scala/Some" | "scala/None$")
}

pub(crate) fn is_list_module_owner(owner: &str) -> bool {
    owner == "scala/collection/immutable/List$"
}

/// How a sequence pattern reads its elements back out.
///
/// scalac wraps the `unapplySeq` result in a value class and calls
/// `lengthCompare$extension` / `apply$extension` / `drop$extension` on it. The
/// wrapper differs for arrays (`scala.Array$UnapplySeqWrapper$`, taking
/// `Object`) and for every `SeqFactory`
/// (`scala.collection.SeqFactory$UnapplySeqWrapper$`, taking `SeqOps`).
/// `List` keeps its own head/tail walk, which the private runtime can back too.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeqPatShape {
    /// `List.unapplySeq`, and any user extractor returning `Option[List[T]]`.
    List,
    /// `Seq` / `Vector` / `IndexedSeq` companions.
    SeqOps,
    /// `scala.Array`.
    Array,
}

pub(crate) fn seq_pat_shape(st: &SymbolTable, uid: SymbolId) -> SeqPatShape {
    let owner = class_internal(st, st.get(uid).owner);
    if owner == SEQPAT_ARRAY_MODULE {
        return SeqPatShape::Array;
    }
    if SEQPAT_SEQOPS_MODULES.contains(&owner.as_str()) {
        return SeqPatShape::SeqOps;
    }
    SeqPatShape::List
}

/// How the `Option`'s payload of a *user-written* `unapplySeq` has to be read.
///
/// `seq_pat_shape` only knows the built-in companions; everything else fell to
/// `SeqPatShape::List`, and the cons walk opens with `checkcast List`. That is
/// right only when the extractor declares `Option[List[A]]`. The natural
/// spelling is `Option[Seq[A]]` -- `Some(s.split(" ").toSeq)` hands back an
/// `ArraySeq$ofRef` -- and the cast blew up at runtime. scalac reads any
/// non-`List` sequence through `SeqFactory$UnapplySeqWrapper$` (an `Array`
/// through `Array$UnapplySeqWrapper$`), which is what those shapes emit.
pub(crate) fn user_unapply_seq_shape(ctx: &EmitCtx, uid: SymbolId) -> SeqPatShape {
    match ctx.st.seq_extractor_payload.get(&uid) {
        Some(SeqPayload::Array) => SeqPatShape::Array,
        Some(SeqPayload::Seq) => SeqPatShape::SeqOps,
        None => SeqPatShape::List,
    }
}

/// Kept in step with `prelude_seqpat::SEQ_FACTORY_MODULES` in the typer;
/// a companion listed there but not here would fall back to the `List` walk
/// and `checkcast` a `Vector` to a `List` at runtime.
pub(crate) const SEQPAT_SEQOPS_MODULES: &[&str] = &[
    "scala/collection/immutable/Seq$",
    "scala/collection/immutable/Vector$",
    "scala/collection/immutable/IndexedSeq$",
];
pub(crate) const SEQPAT_ARRAY_MODULE: &str = "scala/Array$";

/// The class a `Seq` / `Vector` / `IndexedSeq` pattern tests and casts to: the
/// extractor's own parameter class, which is what scalac names too.
pub(crate) fn seq_pat_test_class(ctx: &EmitCtx, param0: Option<&Type>) -> String {
    let name = param0.map(|p| type_jvm_name(ctx.st, p)).unwrap_or_default();
    if name.is_empty() || name == "java/lang/Object" {
        return "scala/collection/SeqOps".into();
    }
    name
}

pub(crate) const SEQOPS_WRAPPER: &str = "scala/collection/SeqFactory$UnapplySeqWrapper$";
pub(crate) const ARRAY_WRAPPER: &str = "scala/Array$UnapplySeqWrapper$";

pub(crate) fn is_stdlib_map(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Map"
            | "scala/collection/immutable/Map$EmptyMap$"
            | "scala/collection/immutable/HashMap"
    )
}

pub(crate) fn is_stdlib_map_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Map$"
}

pub(crate) fn is_stdlib_mapview(owner: &str) -> bool {
    owner == "scala/collection/MapView"
}
pub(crate) fn is_stdlib_vector(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Vector"
            | "scala/collection/immutable/Vector0$"
            | "scala/collection/immutable/Vector1"
            | "scala/collection/immutable/Vector2"
            | "scala/collection/immutable/Vector3"
    )
}

pub(crate) fn is_stdlib_vector_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Vector$"
}

pub(crate) fn is_stdlib_indexedseq(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/IndexedSeq" | "scala/collection/IndexedSeq"
    )
}

pub(crate) fn is_stdlib_indexedseq_module(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/IndexedSeq$" | "scala/collection/IndexedSeq$"
    )
}

pub(crate) fn is_stdlib_queue(owner: &str) -> bool {
    owner == "scala/collection/immutable/Queue"
}

pub(crate) fn is_stdlib_queue_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Queue$"
}

pub(crate) fn is_stdlib_arraybuffer(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayBuffer"
}

pub(crate) fn is_stdlib_arraybuffer_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayBuffer$"
}

pub(crate) fn is_stdlib_mutable_map(owner: &str) -> bool {
    owner == "scala/collection/mutable/Map"
}

pub(crate) fn is_stdlib_mutable_map_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/Map$"
}

pub(crate) fn is_stdlib_mutable_set(owner: &str) -> bool {
    owner == "scala/collection/mutable/Set"
}

pub(crate) fn is_stdlib_mutable_set_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/Set$"
}

pub(crate) fn is_stdlib_coll_iterable(owner: &str) -> bool {
    owner == "scala/collection/Iterable"
}

pub(crate) fn is_stdlib_arraydeque(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayDeque"
}

/// The `scala.collection.mutable` classes whose only constructor takes the
/// initial capacity with a default (`javap`: `public Queue(int)` plus
/// `public static int $lessinit$greater$default$1()`), so `new Queue[A]()`
/// has to fetch the default rather than call a `<init>()V` that is not there.
pub(crate) fn has_default_sized_ctor(internal: &str) -> bool {
    matches!(
        internal,
        "scala/collection/mutable/ArrayDeque"
            | "scala/collection/mutable/Queue"
            | "scala/collection/mutable/Stack"
    )
}

pub(crate) fn is_stdlib_arraydeque_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayDeque$"
}

/// The `scala.collection.mutable` companions declared by
/// `crates/typer/src/prelude_mutcoll.rs`, with the erased descriptor of the
/// inherited `apply` and of the companion's own `empty`. `javap -p` on
/// scala-library-2.13.16: the evidence parameter erases to `Object` for
/// `EvidenceIterableFactory` / `SortedIterableFactory`, but `SortedMapFactory`
/// (`TreeMap$`) declares it as `Ordering`.
pub(crate) fn stdlib_mutcoll_factory(
    owner: &str,
) -> Option<(&'static str, &'static str, &'static str)> {
    match owner {
        "scala/collection/mutable/Queue$" => Some((
            "scala/collection/mutable/Queue",
            "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
            "()Lscala/collection/mutable/Queue;",
        )),
        "scala/collection/mutable/Stack$" => Some((
            "scala/collection/mutable/Stack",
            "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
            "()Lscala/collection/mutable/Stack;",
        )),
        "scala/collection/mutable/ArraySeq$" => Some((
            "scala/collection/mutable/ArraySeq",
            "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
            "(Lscala/reflect/ClassTag;)Lscala/collection/mutable/ArraySeq;",
        )),
        "scala/collection/mutable/TreeSet$" => Some((
            "scala/collection/mutable/TreeSet",
            "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
            "(Lscala/math/Ordering;)Lscala/collection/mutable/TreeSet;",
        )),
        "scala/collection/mutable/PriorityQueue$" => Some((
            "scala/collection/mutable/PriorityQueue",
            "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
            "(Lscala/math/Ordering;)Lscala/collection/mutable/PriorityQueue;",
        )),
        "scala/collection/mutable/TreeMap$" => Some((
            "scala/collection/mutable/TreeMap",
            "(Lscala/collection/immutable/Seq;Lscala/math/Ordering;)Ljava/lang/Object;",
            "(Lscala/math/Ordering;)Lscala/collection/mutable/TreeMap;",
        )),
        _ => None,
    }
}

pub(crate) fn is_stdlib_listbuffer(owner: &str) -> bool {
    owner == "scala/collection/mutable/ListBuffer"
}

pub(crate) fn is_stdlib_listbuffer_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/ListBuffer$"
}

pub(crate) fn is_stdlib_stringbuilder(owner: &str) -> bool {
    owner == "scala/collection/mutable/StringBuilder"
}

pub(crate) fn is_stdlib_hashmap(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashMap"
}

pub(crate) fn is_stdlib_hashmap_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashMap$"
}

pub(crate) fn is_stdlib_hashset(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashSet"
}

pub(crate) fn is_stdlib_hashset_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashSet$"
}

pub(crate) fn is_stdlib_linkedhashmap(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashMap"
}

pub(crate) fn is_stdlib_linkedhashmap_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashMap$"
}

pub(crate) fn is_stdlib_linkedhashset(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashSet"
}

pub(crate) fn is_stdlib_linkedhashset_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashSet$"
}

pub(crate) fn emit_long_numeric_range(asm: &mut Assembler, inclusive: bool) {
    // stack: start (J), end (J). RichLong has no to$extension; IntegralProxy
    // default builds a real NumericRange[Long] via Numeric$LongIsIntegral$.
    emit_box(asm, &Type::Long);
    // start (J), boxedEnd
    asm.dup_x2();
    asm.pop();
    emit_box(asm, &Type::Long);
    asm.swap();
    asm.getstatic(
        "scala/collection/immutable/NumericRange$",
        "MODULE$",
        "Lscala/collection/immutable/NumericRange$;",
    );
    asm.dup_x2();
    asm.pop();
    asm.lconst(1);
    emit_box(asm, &Type::Long);
    asm.getstatic(
        "scala/math/Numeric$LongIsIntegral$",
        "MODULE$",
        "Lscala/math/Numeric$LongIsIntegral$;",
    );
    if inclusive {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "inclusive",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Inclusive;",
        );
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "apply",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Exclusive;",
        );
    }
}

pub(crate) fn emit_integral_numeric_range(asm: &mut Assembler, elem: &Type, inclusive: bool) {
    // stack: start, end (integral as int)
    let integral = match elem {
        Type::Short => "scala/math/Numeric$ShortIsIntegral$",
        Type::Char => "scala/math/Numeric$CharIsIntegral$",
        _ => "scala/math/Numeric$ByteIsIntegral$",
    };
    asm.swap();
    narrow_integral(asm, elem);
    emit_box(asm, elem);
    asm.swap();
    narrow_integral(asm, elem);
    emit_box(asm, elem);
    asm.getstatic(
        "scala/collection/immutable/NumericRange$",
        "MODULE$",
        "Lscala/collection/immutable/NumericRange$;",
    );
    asm.dup_x2();
    asm.pop();
    asm.iconst(1);
    narrow_integral(asm, elem);
    emit_box(asm, elem);
    asm.getstatic(integral, "MODULE$", &format!("L{integral};"));
    if inclusive {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "inclusive",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Inclusive;",
        );
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "apply",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Exclusive;",
        );
    }
}

pub(crate) fn narrow_integral(asm: &mut Assembler, elem: &Type) {
    match elem {
        Type::Short => asm.i2s(),
        Type::Char => asm.i2c(),
        _ => asm.i2b(),
    }
}

pub(crate) fn is_stdlib_range(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Range" | "scala/collection/immutable/Range$Inclusive"
    )
}

pub(crate) fn is_stdlib_numeric_range(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/NumericRange"
            | "scala/collection/immutable/NumericRange$Inclusive"
            | "scala/collection/immutable/NumericRange$Exclusive"
    )
}

pub(crate) fn is_stdlib_set(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Set"
            | "scala/collection/immutable/Set$EmptySet$"
            | "scala/collection/immutable/Set$Set1"
            | "scala/collection/immutable/Set$Set2"
            | "scala/collection/immutable/Set$Set3"
            | "scala/collection/immutable/Set$Set4"
            | "scala/collection/immutable/HashSet"
            | "scala/collection/immutable/SortedSet"
            | "scala/collection/immutable/TreeSet"
    )
}

pub(crate) fn is_stdlib_set_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Set$"
}

pub(crate) fn is_stdlib_sortedset_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/SortedSet$"
}

pub(crate) fn is_stdlib_treeset_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/TreeSet$"
}

pub(crate) fn is_stdlib_sortedmap(owner: &str) -> bool {
    owner == "scala/collection/immutable/SortedMap"
}

pub(crate) fn is_stdlib_sortedmap_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/SortedMap$"
}

pub(crate) fn is_stdlib_treemap(owner: &str) -> bool {
    owner == "scala/collection/immutable/TreeMap"
}

pub(crate) fn is_stdlib_treemap_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/TreeMap$"
}

pub(crate) fn is_stdlib_bitset(owner: &str) -> bool {
    owner == "scala/collection/immutable/BitSet"
}

pub(crate) fn is_stdlib_bitset_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/BitSet$"
}

pub(crate) fn is_stdlib_seq(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Seq" | "scala/collection/Seq"
    )
}

pub(crate) fn is_stdlib_seq_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Seq$"
}

pub(crate) fn is_stdlib_iterable_module(owner: &str) -> bool {
    owner == "scala/collection/Iterable$"
}

pub(crate) fn is_stdlib_lazylist(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/LazyList" | "scala/collection/immutable/LazyList$Empty$"
    )
}

pub(crate) fn is_stdlib_lazylist_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/LazyList$"
}

pub(crate) fn is_stdlib_either(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/Either" | "scala/util/Left" | "scala/util/Right"
    )
}

pub(crate) fn is_stdlib_either_module(owner: &str) -> bool {
    matches!(owner, "scala/util/Left$" | "scala/util/Right$")
}

pub(crate) fn is_stdlib_breaks(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/control/Breaks" | "scala/util/control/Breaks$"
    )
}

pub(crate) fn is_stdlib_try(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/Try" | "scala/util/Success" | "scala/util/Failure"
    )
}

pub(crate) fn is_stdlib_try_module(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/Try$" | "scala/util/Success$" | "scala/util/Failure$"
    )
}
