use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// `reflect_context_stub` asks for the placeholder `blackbox.Context` /
/// `whitebox.Context` of `crate::prelude_reflect`. It is wanted only when the
/// real ones are *not* reachable: scala-reflect.jar on the classpath carries a
/// `Context` with every member on it, and the stub would shadow it.
pub fn install_prelude(st: &mut SymbolTable, library_abi: bool, reflect_context_stub: bool) {
    let root = st.root;
    st.scala_pkg = st.alloc("scala", root, SymKind::Package, Flags::PACKAGE, "scala");
    let java = st.alloc("java", root, SymKind::Package, Flags::PACKAGE, "java");
    let java_lang = st.alloc("lang", java, SymKind::Package, Flags::PACKAGE, "java/lang");

    st.any_sym = class(st, st.scala_pkg, "Any", "java/lang/Object", &[]);
    st.anyref_sym = class(st, st.scala_pkg, "AnyRef", "java/lang/Object", &[Type::Any]);
    st.anyval_sym = class(st, st.scala_pkg, "AnyVal", "java/lang/Object", &[Type::Any]);
    st.object_sym = class(st, java_lang, "Object", "java/lang/Object", &[Type::AnyRef]);
    mark_java(st, st.object_sym);

    st.unit_sym = class(
        st,
        st.scala_pkg,
        "Unit",
        "scala/runtime/BoxedUnit",
        &[Type::AnyVal],
    );
    st.boolean_sym = class(
        st,
        st.scala_pkg,
        "Boolean",
        "java/lang/Boolean",
        &[Type::AnyVal],
    );
    st.int_sym = class(
        st,
        st.scala_pkg,
        "Int",
        "java/lang/Integer",
        &[Type::AnyVal],
    );
    st.long_sym = class(st, st.scala_pkg, "Long", "java/lang/Long", &[Type::AnyVal]);
    st.float_sym = class(
        st,
        st.scala_pkg,
        "Float",
        "java/lang/Float",
        &[Type::AnyVal],
    );
    st.double_sym = class(
        st,
        st.scala_pkg,
        "Double",
        "java/lang/Double",
        &[Type::AnyVal],
    );
    st.char_sym = class(
        st,
        st.scala_pkg,
        "Char",
        "java/lang/Character",
        &[Type::AnyVal],
    );
    // Like `Int`/`Long` above, the JVM name of a primitive value class is the
    // *box* it erases to, not a class of its own: `scala/Byte` does not exist
    // at runtime, so a member call on a `Byte` came out as `invokevirtual
    // scala/Byte.toInt` and the verifier rejected the `int` receiver.
    st.byte_sym = class(st, st.scala_pkg, "Byte", "java/lang/Byte", &[Type::AnyVal]);
    st.short_sym = class(
        st,
        st.scala_pkg,
        "Short",
        "java/lang/Short",
        &[Type::AnyVal],
    );
    // nsc `Byte.+` / `Short.+` / `Char.+` return Int; used by ArrayOps.map(_ + 1).
    method(
        st,
        st.byte_sym,
        "+",
        vec![Type::Int],
        Type::Int,
        Intrinsic::IntBin("+"),
    );
    method(
        st,
        st.short_sym,
        "+",
        vec![Type::Int],
        Type::Int,
        Intrinsic::IntBin("+"),
    );
    method(
        st,
        st.char_sym,
        "+",
        vec![Type::Int],
        Type::Int,
        Intrinsic::IntBin("+"),
    );

    st.string_sym = class(st, java_lang, "String", "java/lang/String", &[Type::AnyRef]);
    mark_java(st, st.string_sym);
    let throwable = class(
        st,
        java_lang,
        "Throwable",
        "java/lang/Throwable",
        &[Type::AnyRef],
    );
    mark_java(st, throwable);
    let throwable_ty = Type::Class {
        sym: throwable,
        args: vec![],
    };
    // `java.lang.Throwable`'s public constructors and the handful of methods
    // slick code actually calls (`getMessage`, `getCause`, `printStackTrace`).
    // Verified against `javap -p java.lang.Throwable`. `Exception` /
    // `RuntimeException` get their own copies of the same four constructor
    // shapes below — constructors are not inherited in Java/Scala, so relying
    // on `lookup_member`'s parent walk here (correct for ordinary methods)
    // would be wrong even though these three classes happen to share the
    // same shapes. `getMessage` etc. *are* ordinary inherited methods, so
    // they only need to exist once, on `Throwable`.
    for params in [
        vec![],
        vec![Type::String],
        vec![Type::String, throwable_ty.clone()],
        vec![throwable_ty.clone()],
    ] {
        method(
            st,
            throwable,
            "<init>",
            params,
            throwable_ty.clone(),
            Intrinsic::None,
        );
    }
    method(
        st,
        throwable,
        "getMessage",
        vec![],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        throwable,
        "getLocalizedMessage",
        vec![],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        throwable,
        "getCause",
        vec![],
        throwable_ty.clone(),
        Intrinsic::None,
    );
    method(
        st,
        throwable,
        "initCause",
        vec![throwable_ty.clone()],
        throwable_ty.clone(),
        Intrinsic::None,
    );
    method(
        st,
        throwable,
        "printStackTrace",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let exception = class(
        st,
        java_lang,
        "Exception",
        "java/lang/Exception",
        &[throwable_ty.clone()],
    );
    mark_java(st, exception);
    let exception_ty = Type::Class {
        sym: exception,
        args: vec![],
    };
    for params in [
        vec![],
        vec![Type::String],
        vec![Type::String, throwable_ty.clone()],
        vec![throwable_ty.clone()],
    ] {
        method(
            st,
            exception,
            "<init>",
            params,
            exception_ty.clone(),
            Intrinsic::None,
        );
    }
    let _runtime_ex = class(
        st,
        java_lang,
        "RuntimeException",
        "java/lang/RuntimeException",
        &[exception_ty],
    );
    mark_java(st, _runtime_ex);
    let runtime_ex_ty = Type::Class {
        sym: _runtime_ex,
        args: vec![],
    };
    for params in [
        vec![],
        vec![Type::String],
        vec![Type::String, throwable_ty.clone()],
        vec![throwable_ty],
    ] {
        method(
            st,
            _runtime_ex,
            "<init>",
            params,
            runtime_ex_ty.clone(),
            Intrinsic::None,
        );
    }
    let jclass = class(st, java_lang, "Class", "java/lang/Class", &[Type::AnyRef]);
    mark_java(st, jclass);
    let class_t = type_param(st, jclass, "T");
    st.get_mut(jclass).tparams = vec![class_t];
    method(st, jclass, "getName", vec![], Type::String, Intrinsic::None);
    st.array_sym = class(
        st,
        st.scala_pkg,
        "Array",
        "[java/lang/Object",
        &[Type::AnyRef],
    );
    st.option_sym = class(st, st.scala_pkg, "Option", "scala/Option", &[Type::AnyRef]);
    st.some_sym = class(
        st,
        st.scala_pkg,
        "Some",
        "scala/Some",
        &[Type::Class {
            sym: st.option_sym,
            args: vec![],
        }],
    );
    st.none_sym = module_extending(
        st,
        st.scala_pkg,
        "None",
        "scala/None$",
        Type::Class {
            sym: st.option_sym,
            args: vec![],
        },
    );
    st.list_sym = class(
        st,
        st.scala_pkg,
        "List",
        "scala/collection/immutable/List",
        &[Type::AnyRef],
    );
    st.nil_sym = module_extending(
        st,
        st.scala_pkg,
        "Nil",
        "scala/collection/immutable/Nil$",
        Type::Class {
            sym: st.list_sym,
            args: vec![],
        },
    );
    st.cons_sym = class(
        st,
        st.scala_pkg,
        "$colon$colon",
        "scala/collection/immutable/$colon$colon",
        &[Type::Class {
            sym: st.list_sym,
            args: vec![],
        }],
    );
    let cons_alias = st.alloc(
        "::",
        st.scala_pkg,
        SymKind::Class,
        Flags::CASE,
        "scala/collection/immutable/$colon$colon",
    );
    st.get_mut(cons_alias).ty = Type::Class {
        sym: st.cons_sym,
        args: vec![],
    };

    add_any_members(st);
    add_int_members(st);
    add_long_members(st);
    add_double_members(st);
    add_float_members(st);
    add_bool_members(st);
    crate::prelude_numeric::install(st, library_abi);
    add_string_members(st, library_abi);
    add_array_members(st);
    let with_filter = add_with_filter(st);
    let option_wf = add_option_with_filter(st);
    let iterator = if library_abi {
        Some(add_iterator(st))
    } else {
        None
    };
    let string_ops = if library_abi {
        Some(add_string_ops(st, iterator.unwrap()))
    } else {
        None
    };
    let array_ops = if library_abi {
        Some(add_array_ops(st))
    } else {
        None
    };
    add_option_members(st, option_wf, library_abi);
    crate::prelude_sgap::fix_option_flat_map(st);
    add_cons_members(st, library_abi);
    crate::prelude_either::install_option_core(st);
    crate::prelude_either::install_java_lang_exceptions(st);
    add_list_members(st, with_filter, iterator, library_abi);
    add_function_types(st);
    add_partial_function(st);
    if library_abi {
        add_list_collect(st);
        let ct = add_classtag(st, jclass);
        if let Some(aops) = array_ops {
            add_array_ops_map(st, aops, ct);
            add_array_ops_flat_map(st, aops, ct);
            add_array_ops_flat_map_from_array(st, aops, ct);
            add_array_ops_collect(st, aops, ct);
        }
        if let Some(so) = string_ops {
            add_string_ops_to_array(st, so, ct);
        }
        add_string_context(st);
        add_array_companion(st, ct);
    }
    let ordered = add_ordered(st);
    add_delayed_init_app(st);

    // Some companion with apply
    let some_mod = module(st, st.scala_pkg, "Some", "scala/Some$");
    let some_cls = st.module_class_of(some_mod);
    method(
        st,
        some_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: st.some_sym,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(some_cls).members.clone();
    st.get_mut(some_mod).members.extend(mems);

    let tuple2 = class(st, st.scala_pkg, "Tuple2", "scala/Tuple2", &[Type::AnyRef]);
    let t2a = type_param(st, tuple2, "A");
    let t2b = type_param(st, tuple2, "B");
    st.get_mut(tuple2).tparams = vec![t2a, t2b];
    let f1 = st.alloc("_1", tuple2, SymKind::Term, Flags::FINAL, "");
    st.get_mut(f1).ty = Type::TypeParam(t2a);
    let f2 = st.alloc("_2", tuple2, SymKind::Term, Flags::FINAL, "");
    st.get_mut(f2).ty = Type::TypeParam(t2b);
    st.get_mut(tuple2).ctor_fields = vec![f1, f2];
    method(
        st,
        tuple2,
        "<init>",
        vec![Type::Any, Type::Any],
        Type::Class {
            sym: tuple2,
            args: vec![],
        },
        Intrinsic::None,
    );
    if let Some(so) = string_ops {
        let pair = Type::Class {
            sym: tuple2,
            args: vec![Type::String, Type::String],
        };
        method(
            st,
            so,
            "span",
            vec![fn1(Type::Char, Type::Boolean)],
            pair.clone(),
            Intrinsic::None,
        );
        method(
            st,
            so,
            "partition",
            vec![fn1(Type::Char, Type::Boolean)],
            pair.clone(),
            Intrinsic::None,
        );
        method(st, so, "splitAt", vec![Type::Int], pair, Intrinsic::None);
    }
    if library_abi {
        if let Some(aops) = array_ops {
            add_array_ops_zip(st, aops, tuple2);
            add_array_ops_folds(st, aops);
            add_array_ops_scan_left(st, aops);
        }
        if let Some(so) = string_ops {
            add_string_ops_fold_left(st, so);
            add_string_ops_fold_right_and_grouped(st, so);
            add_string_ops_map_and_appended(st, so);
        }
    }

    // Marker trait `scala.Dynamic`. JVM interface lives in scala-library.jar;
    // we only need the symbol so `class D extends Dynamic` typechecks.
    let _dynamic = iface(st, st.scala_pkg, "Dynamic", "scala/Dynamic");
    let language = module(st, st.scala_pkg, "language", "scala/language$");
    let lang_cls = st.module_class_of(language);
    let dynamics = st.alloc(
        "dynamics",
        lang_cls,
        SymKind::Term,
        Flags::IMPLICIT.with(Flags::LAZY).with(Flags::FINAL),
        "",
    );
    st.get_mut(dynamics).ty = Type::Boolean;
    st.get_mut(language).members.push(dynamics);
    for feat in ["postfixOps", "implicitConversions"] {
        let id = st.alloc(
            feat,
            lang_cls,
            SymKind::Term,
            Flags::IMPLICIT.with(Flags::LAZY).with(Flags::FINAL),
            "",
        );
        st.get_mut(id).ty = Type::Boolean;
        st.get_mut(language).members.push(id);
    }

    let rich_int = if library_abi {
        Some(add_rich_int_and_range(st))
    } else {
        None
    };
    let rich_ldc = if library_abi {
        Some(add_rich_long_double_char(st))
    } else {
        None
    };
    if library_abi {
        add_map_and_vector(st);
        add_set(st);
        let ordering = add_ordering(st);
        add_sorted_set(st, ordering);
        add_sorted_map(st, ordering);
        add_bit_set(st);
        if let Some(so) = string_ops {
            add_string_ops_sorted(st, so, ordering);
            add_string_ops_indices_and_r(st, so);
            add_string_ops_compare_patch_length(st, so);
            if let Some(it) = iterator {
                add_string_ops_iterator_size_appended(st, so, it);
            }
        }
        if let Some(aops) = array_ops {
            add_array_ops_remaining(st, aops);
            add_array_ops_filter_not_opts_part(st, aops, tuple2);
            add_array_ops_zip_index_size(st, aops, tuple2);
            if let Some(it) = iterator {
                add_array_ops_length_index_copy(st, aops, it);
            }
        }
        if let Some(so) = string_ops {
            add_string_ops_concat_length_flat(st, so);
        }
        add_seq_and_lazylist(st);
        fix_string_context_parts(st);
        add_view(st);
        add_indexedseq_and_queue(st);
        add_array_buffer(st);
        add_list_buffer(st);
        add_array_deque(st);
        add_string_builder(st);
        add_hash_map(st);
        add_hash_set(st);
        if let Some(it) = iterator {
            crate::prelude_coll::add_collections_extra(st, tuple2, ordering, it);
        }
        crate::prelude_sgap::add_iterable_apply(st, library_abi);
        if let Some(aops) = array_ops {
            // ArrayOps の変換・集約系 (toList/toSeq/groupBy/sum/...) と
            // scala.collection.MapView。Buffer / Iterable / MapView を
            // 作り直さないよう、コレクション本体のあとに走らせる。
            crate::prelude_arrconv::install(st, aops, tuple2, ordering);
        }
        add_linked_hash_map(st);
        add_linked_hash_set(st);
        add_either(st);
        add_try(st, throwable);
        crate::prelude_either::install_library_abi(st);
        add_breaks(st);
        add_big_int(st);
        add_big_decimal(st);
        crate::prelude_oshadow::install(st);
        add_chaining(st);
        add_using(st);
        add_xml(st);
        add_enumeration(st);
        crate::prelude_enum::install(st);
    }
    crate::prelude_seq::add_list_core(st, library_abi);
    crate::prelude_text::install(st, library_abi);
    crate::prelude_strhier::install_string_search(st);
    crate::prelude_ovl2::install(st);
    add_annotation_pkg(st);
    add_java_sam(st, java, java_lang);

    let arrow = if library_abi {
        let a = class(
            st,
            st.scala_pkg,
            "ArrowAssoc",
            "scala/Predef$ArrowAssoc",
            &[Type::AnyVal],
        );
        let af = st.alloc("self", a, SymKind::Term, Flags::PARAM, "");
        st.get_mut(af).ty = Type::Any;
        st.get_mut(a).ctor_fields = vec![af];
        method(
            st,
            a,
            "->",
            vec![Type::Any],
            Type::Class {
                sym: tuple2,
                args: vec![Type::Any, Type::Any],
            },
            Intrinsic::None,
        );
        a
    } else {
        let a = class(
            st,
            st.scala_pkg,
            "ArrowAssoc",
            "scala/runtime/ArrowAssoc",
            &[Type::AnyRef],
        );
        let af = st.alloc("self", a, SymKind::Term, Flags::PARAM, "");
        st.get_mut(af).ty = Type::Any;
        st.get_mut(a).ctor_fields = vec![af];
        method(
            st,
            a,
            "->",
            vec![Type::Any],
            Type::Class {
                sym: tuple2,
                args: vec![Type::Any, Type::Any],
            },
            Intrinsic::None,
        );
        a
    };

    st.predef = module(st, st.scala_pkg, "Predef", "scala/Predef$");
    add_predef_members(
        st,
        arrow,
        string_ops,
        array_ops,
        rich_int,
        rich_ldc,
        library_abi,
    );

    crate::prelude_lowbound::install(st);
    crate::prelude_lang::install(st);
    crate::prelude_lazyref::install(st);

    st.push_scope();
    st.enter_in_current("scala", st.scala_pkg);
    st.enter_in_current("java", java);
    if reflect_context_stub {
        crate::prelude_reflect::install_reflect_macros(st);
    }
    import_members(st, st.scala_pkg);
    import_members(st, java_lang);
    import_members(st, st.predef);
    st.enter_in_current("String", st.string_sym);
    st.enter_in_current("Unit", st.unit_sym);
    crate::prelude_tuple::add_tuples(st, library_abi);
    add_package_paths(st);
    add_scala_aliases(st, library_abi);
    st.enter_in_current("::", st.cons_sym);
    st.enter_in_current("Ordered", ordered);
    crate::prelude_numops::install(st);
    crate::prelude_mutcoll::install(st, library_abi);
    crate::prelude_mutops::install(st);
    crate::prelude_regex::install(st, library_abi);
    crate::prelude_ordtuple::install(st, library_abi);
    crate::prelude_universal::install(st);
    crate::prelude_bsops::install(st);
    crate::prelude_numconv::install(st);
    crate::prelude_numhier::install(st, library_abi);
    crate::prelude_variance::install(st);
    crate::prelude_boxed::install(st);
    crate::prelude_hier::install(st);
    // `Seq[A] <: PartialFunction[Int, A] <: Int => A`（`scala/collection/Seq`
    // は `prelude_hier` が組み立てるので、そのあと）。`PartialFunction` の
    // `lift`/`orElse` も同じスライスで足す。library_abi 専用。
    crate::prelude_seqfn::install(st, library_abi);
    crate::prelude_fntuple::install(st, library_abi);
    crate::prelude_mism9::install(st);
    if library_abi {
        crate::prelude_durrange::install_range_companion(st);
        crate::prelude_durrange::install_ordered_companion(st);
    }
    // `val Ordering = scala.math.Ordering`（項位置の別名）。コンパニオンが
    // 出そろってから入れるので最後。
    crate::prelude_ordsummon::install(st, library_abi);
    // `Ordering[T] <: PartialOrdering[T] <: Equiv[T]` と `object Equiv` の
    // implicit instance。`Equiv` のコンパニオン別名が上の行で入ってから。
    crate::prelude_eqtail::install(st, library_abi);
}

/// Prelude classes are owned by `scala` but carry their real JVM package
/// (`scala/util/Try`), so `scala.util.Try` did not resolve. Register each one
/// under the package its JVM name names, leaving its owner alone.
fn add_package_paths(st: &mut SymbolTable) {
    let ids: Vec<SymbolId> = (0..st.symbols.len())
        .map(|i| SymbolId(i as u32))
        .filter(|id| {
            let s = st.get(*id);
            matches!(s.kind, SymKind::Class | SymKind::Module)
                && s.jvm_name.matches('/').count() >= 2
                // A module's `Try$` is fine; a nested `Try$WithFilter` is not.
                && !s.jvm_name.trim_end_matches('$').contains('$')
                // `scala.Long`'s JVM name is the *box* it erases to, not a
                // package path: entering it as `java.lang.Long` made
                // `java.util.ArrayList[java.lang.Long]` an `ArrayList[Long]`
                // that `add(7L)` could not satisfy.
                && !st.is_primitive_value_class(*id)
        })
        .collect();
    for id in ids {
        let jvm = st.get(id).jvm_name.clone();
        let Some((pkg, _)) = jvm.rsplit_once('/') else {
            continue;
        };
        if pkg == "scala" || pkg.is_empty() {
            continue;
        }
        let name = st.get(id).name.clone();
        let p = crate::classpath::ensure_package(st, pkg);
        if p == st.root || st.get(p).members.contains(&id) {
            continue;
        }
        // Both the class and its companion belong under the package; term
        // position picks the module.
        if !st
            .lookup_member(p, &name)
            .into_iter()
            .any(|s| st.get(s).kind == st.get(id).kind)
        {
            st.get_mut(p).members.push(id);
        }
    }
}

/// nsc: the `scala` package object aliases these, so `Ordering[Int]` resolves
/// without an import. Entering the class makes the alias usable in type
/// position; a companion of the same name stays reachable in term position.
fn add_scala_aliases(st: &mut SymbolTable, library_abi: bool) {
    for (name, jvm) in [
        ("Ordering", "scala/math/Ordering"),
        ("Numeric", "scala/math/Numeric"),
        ("BigInt", "scala/math/BigInt"),
        ("BigDecimal", "scala/math/BigDecimal"),
        ("Equiv", "scala/math/Equiv"),
        ("Fractional", "scala/math/Fractional"),
        ("Integral", "scala/math/Integral"),
    ] {
        let Some(cls) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        let already = st
            .lookup(name)
            .into_iter()
            .any(|s| s == cls && st.get(s).kind == SymKind::Class);
        if !already {
            st.enter_in_current(name, cls);
        }
    }
    // Must come after the block above: it enters `<:<`/`=:=`/`Iterable`/
    // `IterableOnce` into this same base scope (the one that stays open for
    // the whole compilation), the same way `Ordered` just did on the line
    // above — those types own no package that the earlier `import_members`
    // calls would have picked up, and without an explicit scope entry
    // `expose_unqualified` (check.rs) can't resolve them unqualified from
    // user source at all (it only probes for a *root-level* classfile by
    // that literal name, which doesn't exist for anything under
    // `scala/collection/`).
    crate::prelude_conform::install(st, library_abi);
    // Needs `<:<` (installed just above) and `Map`.
    crate::prelude_impl2::install(st, library_abi);
    // `Map[K, V] <: K => V`。階層表のあとに張る。
    crate::prelude_mism4::install(st);
    // `case Seq(a, b)` / `case Array(a, b)`。companion が揃ってから足す。
    crate::prelude_seqpat::install(st);
    // `StringOps.map[B](Char => B): IndexedSeq[B]`（`IndexedSeq` が揃ってから）。
    crate::prelude_strmap::install(st, library_abi);
    // `StringOps` の残り。pickle から補完できないもの（戻り型だけのオーバー
    // ロードなど）だけを手書きする。
    crate::prelude_stringops8::install(st, library_abi);
    // `Coll.empty` は最後にまとめて多相化する（すべての companion が揃ってから）。
    crate::prelude_empty::install(st);
}

fn mark_java(st: &mut SymbolTable, id: SymbolId) {
    let f = st.get(id).flags.with(Flags::JAVA);
    st.get_mut(id).flags = f;
}

/// SIP-21 Java SAM types: `Runnable`, `Comparator[T]`, `java.util.function.Function`.
fn add_java_sam(st: &mut SymbolTable, java: SymbolId, java_lang: SymbolId) {
    let runnable = iface(st, java_lang, "Runnable", "java/lang/Runnable");
    mark_java(st, runnable);
    let run = st.alloc("run", runnable, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(run).ty = Type::Method {
        paramss: Vec::new(),
        ret: Box::new(Type::Unit),
    };

    let util = st.alloc("util", java, SymKind::Package, Flags::PACKAGE, "java/util");
    let comparator = iface(st, util, "Comparator", "java/util/Comparator");
    mark_java(st, comparator);
    let ct = type_param(st, comparator, "T");
    st.get_mut(comparator).tparams = vec![ct];
    let cmp = st.alloc("compare", comparator, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(cmp).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ct), Type::TypeParam(ct)]],
        ret: Box::new(Type::Int),
    };

    let fn_pkg = st.alloc(
        "function",
        util,
        SymKind::Package,
        Flags::PACKAGE,
        "java/util/function",
    );
    let jfun = iface(st, fn_pkg, "Function", "java/util/function/Function");
    mark_java(st, jfun);
    let ft = type_param(st, jfun, "T");
    let fr = type_param(st, jfun, "R");
    st.get_mut(jfun).tparams = vec![ft, fr];
    let apply = st.alloc("apply", jfun, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ft)]],
        ret: Box::new(Type::TypeParam(fr)),
    };
}

fn add_annotation_pkg(st: &mut SymbolTable) {
    let pkg = st.alloc(
        "annotation",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/annotation",
    );
    let annotation = abs_class(
        st,
        pkg,
        "Annotation",
        "scala/annotation/Annotation",
        &[Type::AnyRef],
    );
    let static_annot = abs_class(
        st,
        pkg,
        "StaticAnnotation",
        "scala/annotation/StaticAnnotation",
        &[Type::Class {
            sym: annotation,
            args: vec![],
        }],
    );
    let _ = abs_class(
        st,
        pkg,
        "switch",
        "scala/annotation/switch",
        &[Type::Class {
            sym: static_annot,
            args: vec![],
        }],
    );
    let inf = class(
        st,
        pkg,
        "implicitNotFound",
        "scala/annotation/implicitNotFound",
        &[Type::Class {
            sym: static_annot,
            args: vec![],
        }],
    );
    let inf_msg = st.alloc("msg", inf, SymKind::Term, Flags::PARAM, "");
    st.get_mut(inf_msg).ty = Type::String;
    st.get_mut(inf).ctor_fields = vec![inf_msg];
    let unc_pkg = st.alloc(
        "unchecked",
        pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/annotation/unchecked",
    );
    let _ = abs_class(
        st,
        unc_pkg,
        "uncheckedVariance",
        "scala/annotation/unchecked/uncheckedVariance",
        &[Type::Class {
            sym: static_annot,
            args: vec![],
        }],
    );
    let static_t = Type::Class {
        sym: static_annot,
        args: vec![],
    };
    // nsc: `scala.inline` / `scala.noinline` / `scala.volatile` / `scala.transient`
    for (name, jvm) in [
        ("inline", "scala/inline"),
        ("noinline", "scala/noinline"),
        ("volatile", "scala/volatile"),
        ("transient", "scala/transient"),
        ("native", "scala/native"),
    ] {
        let _ = abs_class(st, st.scala_pkg, name, jvm, &[static_t.clone()]);
    }
}

pub(crate) fn class(
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

pub(crate) fn iface(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    let id = st.alloc(
        name,
        owner,
        SymKind::Class,
        Flags::INTERFACE.with(Flags::ABSTRACT).with(Flags::TRAIT),
        jvm,
    );
    st.get_mut(id).parents = vec![Type::AnyRef];
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}

pub(crate) fn module(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    let cls = st.alloc(
        &format!("{name}$"),
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

pub(crate) fn module_extending(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parent: Type,
) -> SymbolId {
    let m = module(st, owner, name, jvm);
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![parent];
    m
}

pub(crate) fn method(
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

/// `method` for sibling prelude modules.
pub(crate) fn prelude_method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
    intrinsic: Intrinsic,
) -> SymbolId {
    method(st, owner, name, params, ret, intrinsic)
}

fn add_ordered(st: &mut SymbolTable) -> SymbolId {
    let math = st.alloc(
        "math",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/math",
    );
    let ordered = iface(st, math, "Ordered", "scala/math/Ordered");
    let a = type_param(st, ordered, "A");
    st.get_mut(ordered).tparams = vec![a];
    let cmp = st.alloc("compare", ordered, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(cmp).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(a)]],
        ret: Box::new(Type::Int),
    };
    for op in ["<", ">", "<=", ">="] {
        let id = st.alloc(op, ordered, SymKind::Method, Flags::EMPTY, "");
        st.get_mut(id).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(a)]],
            ret: Box::new(Type::Boolean),
        };
    }
    ordered
}

/// `scala.math.Ordering` + companion `implicit object Int` (`Ordering$Int$.MODULE$`).
fn add_ordering(st: &mut SymbolTable) -> SymbolId {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let ordering = iface(st, math, "Ordering", "scala/math/Ordering");
    let t = type_param(st, ordering, "T");
    st.get_mut(ordering).tparams = vec![t];
    method(
        st,
        ordering,
        "compare",
        vec![Type::Any, Type::Any],
        Type::Int,
        Intrinsic::None,
    );
    let ord_mod = module(st, math, "Ordering", "scala/math/Ordering$");
    let ord_cls = st.module_class_of(ord_mod);
    add_ordering_instance(
        st,
        ord_cls,
        ordering,
        "Int",
        "scala/math/Ordering$Int$",
        Type::Int,
    );
    add_ordering_instance(
        st,
        ord_cls,
        ordering,
        "Char",
        "scala/math/Ordering$Char$",
        Type::Char,
    );
    let mems = st.get(ord_cls).members.clone();
    st.get_mut(ord_mod).members.extend(mems);
    ordering
}

fn add_ordering_instance(
    st: &mut SymbolTable,
    ord_cls: SymbolId,
    ordering: SymbolId,
    name: &str,
    jvm: &str,
    arg: Type,
) {
    let m = module(st, ord_cls, name, jvm);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).ty = Type::Class {
        sym: ordering,
        args: vec![arg.clone()],
    };
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![Type::Class {
        sym: ordering,
        args: vec![arg],
    }];
}

fn add_sorted_factory(st: &mut SymbolTable, owner: SymbolId, cls: SymbolId, ordering: SymbolId) {
    let cls_t = Type::Class {
        sym: cls,
        args: vec![Type::Any],
    };
    let apply = method(
        st,
        owner,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        cls_t,
        Intrinsic::None,
    );
    let aa = type_param(st, apply, "A");
    let xs = st.alloc(
        "elems",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(xs).ty = Type::Repeated(Box::new(Type::TypeParam(aa)));
    let ev = st.alloc(
        "evidence$1",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::TypeParam(aa)],
    };
    st.get_mut(apply).tparams = vec![aa];
    st.get_mut(apply).params = vec![xs, ev];
    st.get_mut(apply).paramss = vec![vec![xs], vec![ev]];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![
            vec![Type::Repeated(Box::new(Type::TypeParam(aa)))],
            vec![Type::Class {
                sym: ordering,
                args: vec![Type::TypeParam(aa)],
            }],
        ],
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(aa)],
        }),
    };
}

fn add_sorted_set(st: &mut SymbolTable, ordering: SymbolId) {
    let immp = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let ss = iface(
        st,
        immp,
        "SortedSet",
        "scala/collection/immutable/SortedSet",
    );
    let sa = type_param(st, ss, "A");
    st.get_mut(ss).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    method(
        st,
        ss,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        ss,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let ss_mod = module(
        st,
        immp,
        "SortedSet",
        "scala/collection/immutable/SortedSet$",
    );
    let ss_cls = st.module_class_of(ss_mod);
    add_sorted_factory(st, ss_cls, ss, ordering);
    let mems = st.get(ss_cls).members.clone();
    st.get_mut(ss_mod).members.extend(mems);

    let ts = class(
        st,
        immp,
        "TreeSet",
        "scala/collection/immutable/TreeSet",
        &[Type::Class {
            sym: ss,
            args: vec![],
        }],
    );
    let tsa = type_param(st, ts, "A");
    st.get_mut(ts).tparams = vec![tsa];
    let tta = Type::TypeParam(tsa);
    st.get_mut(ts).parents = vec![Type::Class {
        sym: ss,
        args: vec![tta.clone()],
    }];
    method(
        st,
        ts,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        ts,
        "foreach",
        vec![fn1(tta, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let ts_mod = module(st, immp, "TreeSet", "scala/collection/immutable/TreeSet$");
    let ts_cls = st.module_class_of(ts_mod);
    add_sorted_factory(st, ts_cls, ts, ordering);
    let tmems = st.get(ts_cls).members.clone();
    st.get_mut(ts_mod).members.extend(tmems);
}

fn add_sorted_map_factory(
    st: &mut SymbolTable,
    owner: SymbolId,
    cls: SymbolId,
    ordering: SymbolId,
    tuple2: SymbolId,
) {
    let apply = method(
        st,
        owner,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::Class {
            sym: cls,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let k = type_param(st, apply, "K");
    let v = type_param(st, apply, "V");
    let pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(k), Type::TypeParam(v)],
    };
    let xs = st.alloc(
        "elems",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(xs).ty = Type::Repeated(Box::new(pair.clone()));
    let ev = st.alloc(
        "evidence$1",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::TypeParam(k)],
    };
    st.get_mut(apply).tparams = vec![k, v];
    st.get_mut(apply).params = vec![xs, ev];
    st.get_mut(apply).paramss = vec![vec![xs], vec![ev]];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![
            vec![Type::Repeated(Box::new(pair))],
            vec![Type::Class {
                sym: ordering,
                args: vec![Type::TypeParam(k)],
            }],
        ],
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(k), Type::TypeParam(v)],
        }),
    };
}

fn add_sorted_map(st: &mut SymbolTable, ordering: SymbolId) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let immp = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let sm = iface(
        st,
        immp,
        "SortedMap",
        "scala/collection/immutable/SortedMap",
    );
    let sk = type_param(st, sm, "K");
    let sv = type_param(st, sm, "V");
    st.get_mut(sm).tparams = vec![sk, sv];
    let tk = Type::TypeParam(sk);
    let tv = Type::TypeParam(sv);
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };
    method(
        st,
        sm,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        sm,
        "get",
        vec![Type::Any],
        Type::Class {
            sym: st.option_sym,
            args: vec![tv.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        sm,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let sm_mod = module(
        st,
        immp,
        "SortedMap",
        "scala/collection/immutable/SortedMap$",
    );
    let sm_cls = st.module_class_of(sm_mod);
    add_sorted_map_factory(st, sm_cls, sm, ordering, tuple2);
    let mems = st.get(sm_cls).members.clone();
    st.get_mut(sm_mod).members.extend(mems);

    let tm = class(
        st,
        immp,
        "TreeMap",
        "scala/collection/immutable/TreeMap",
        &[Type::Class {
            sym: sm,
            args: vec![],
        }],
    );
    let tmk = type_param(st, tm, "K");
    let tmv = type_param(st, tm, "V");
    st.get_mut(tm).tparams = vec![tmk, tmv];
    let ttk = Type::TypeParam(tmk);
    let ttv = Type::TypeParam(tmv);
    st.get_mut(tm).parents = vec![Type::Class {
        sym: sm,
        args: vec![ttk.clone(), ttv.clone()],
    }];
    let tpair = Type::Class {
        sym: tuple2,
        args: vec![ttk.clone(), ttv.clone()],
    };
    method(
        st,
        tm,
        "apply",
        vec![Type::Any],
        ttv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        tm,
        "get",
        vec![Type::Any],
        Type::Class {
            sym: st.option_sym,
            args: vec![ttv],
        },
        Intrinsic::None,
    );
    method(
        st,
        tm,
        "foreach",
        vec![fn1(tpair, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let tm_mod = module(st, immp, "TreeMap", "scala/collection/immutable/TreeMap$");
    let tm_cls = st.module_class_of(tm_mod);
    add_sorted_map_factory(st, tm_cls, tm, ordering, tuple2);
    let tmems = st.get(tm_cls).members.clone();
    st.get_mut(tm_mod).members.extend(tmems);
}

pub(crate) fn type_param(st: &mut SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    id
}

fn import_members(st: &mut SymbolTable, owner: SymbolId) {
    let members = st.get(owner).members.clone();
    for m in members {
        let name = st.get(m).name.clone();
        if name.ends_with('$') {
            continue;
        }
        st.enter_in_current(&name, m);
    }
}

fn add_any_members(st: &mut SymbolTable) {
    let any = st.any_sym;
    method(
        st,
        any,
        "==",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::AnyEq,
    );
    method(
        st,
        any,
        "!=",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::AnyNe,
    );
    method(
        st,
        any,
        "equals",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, any, "hashCode", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        any,
        "toString",
        vec![],
        Type::String,
        Intrinsic::AnyToString,
    );
    // nsc `Any.asInstanceOf[T0]: T0` / `Any.isInstanceOf[T0]: Boolean` are
    // generic over the explicit type argument: `x.asInstanceOf[String]` must
    // type as `String`, not `Any`.
    let as_instance_of = method(
        st,
        any,
        "asInstanceOf",
        vec![],
        Type::Any,
        Intrinsic::AsInstanceOf,
    );
    let aio_t = type_param(st, as_instance_of, "T0");
    st.get_mut(as_instance_of).tparams = vec![aio_t];
    st.get_mut(as_instance_of).ty = Type::Method {
        paramss: Vec::new(),
        ret: Box::new(Type::TypeParam(aio_t)),
    };
    let is_instance_of = method(
        st,
        any,
        "isInstanceOf",
        vec![],
        Type::Boolean,
        Intrinsic::IsInstanceOf,
    );
    let iio_t = type_param(st, is_instance_of, "T0");
    st.get_mut(is_instance_of).tparams = vec![iio_t];
    // nsc `Any.synchronized[T0](body: => T0): T0`
    let sync = method(
        st,
        any,
        "synchronized",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::Synchronized,
    );
    let t0 = type_param(st, sync, "T0");
    st.get_mut(sync).tparams = vec![t0];
    st.get_mut(sync).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::TypeParam(t0)))]],
        ret: Box::new(Type::TypeParam(t0)),
    };
    let anyref = st.anyref_sym;
    method(
        st,
        anyref,
        "eq",
        vec![Type::AnyRef],
        Type::Boolean,
        Intrinsic::Eq,
    );
    method(
        st,
        anyref,
        "ne",
        vec![Type::AnyRef],
        Type::Boolean,
        Intrinsic::Ne,
    );
}

fn add_int_members(st: &mut SymbolTable) {
    let c = st.int_sym;
    for (op, ic) in [
        ("+", Intrinsic::IntBin("+")),
        ("-", Intrinsic::IntBin("-")),
        ("*", Intrinsic::IntBin("*")),
        ("/", Intrinsic::IntBin("/")),
        ("%", Intrinsic::IntBin("%")),
        ("&", Intrinsic::IntBin("&")),
        ("|", Intrinsic::IntBin("|")),
        ("^", Intrinsic::IntBin("^")),
        ("<<", Intrinsic::IntBin("<<")),
        (">>", Intrinsic::IntBin(">>")),
        (">>>", Intrinsic::IntBin(">>>")),
    ] {
        method(st, c, op, vec![Type::Int], Type::Int, ic);
    }
    for (op, ic) in [
        ("==", Intrinsic::IntBin("==")),
        ("!=", Intrinsic::IntBin("!=")),
        ("<", Intrinsic::IntBin("<")),
        ("<=", Intrinsic::IntBin("<=")),
        (">", Intrinsic::IntBin(">")),
        (">=", Intrinsic::IntBin(">=")),
    ] {
        method(st, c, op, vec![Type::Int], Type::Boolean, ic);
    }
    method(st, c, "unary_-", vec![], Type::Int, Intrinsic::IntUn("-"));
    method(st, c, "unary_~", vec![], Type::Int, Intrinsic::IntUn("~"));
    method(st, c, "toLong", vec![], Type::Long, Intrinsic::IntToLong);
    method(st, c, "toFloat", vec![], Type::Float, Intrinsic::IntToFloat);
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::IntToDouble,
    );
    method(st, c, "toByte", vec![], Type::Byte, Intrinsic::IntToByte);
    method(st, c, "toShort", vec![], Type::Short, Intrinsic::IntToShort);
    method(
        st,
        c,
        "toString",
        vec![],
        Type::String,
        Intrinsic::AnyToString,
    );
    method(
        st,
        c,
        "+",
        vec![Type::Long],
        Type::Long,
        Intrinsic::LongBin("+"),
    );
    method(
        st,
        c,
        "+",
        vec![Type::Double],
        Type::Double,
        Intrinsic::DoubleBin("+"),
    );
}

fn add_long_members(st: &mut SymbolTable) {
    let c = st.long_sym;
    for (op, ic) in [
        ("+", Intrinsic::LongBin("+")),
        ("-", Intrinsic::LongBin("-")),
        ("*", Intrinsic::LongBin("*")),
        ("/", Intrinsic::LongBin("/")),
        ("%", Intrinsic::LongBin("%")),
    ] {
        method(st, c, op, vec![Type::Long], Type::Long, ic);
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Long],
            Type::Boolean,
            Intrinsic::LongBin(op),
        );
    }
    method(st, c, "unary_-", vec![], Type::Long, Intrinsic::LongUn("-"));
    method(st, c, "unary_~", vec![], Type::Long, Intrinsic::LongUn("~"));
    // `Intrinsic::None` here boxed the receiver and called a `java.lang.Long`
    // method that does not exist; `l2i` is the whole of `Long.toInt`.
    method(st, c, "toInt", vec![], Type::Int, Intrinsic::NumConv("JI"));
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::LongToDouble,
    );
    method(
        st,
        c,
        "toFloat",
        vec![],
        Type::Float,
        Intrinsic::LongToFloat,
    );
}

fn add_double_members(st: &mut SymbolTable) {
    let c = st.double_sym;
    for (op, ic) in [
        ("+", Intrinsic::DoubleBin("+")),
        ("-", Intrinsic::DoubleBin("-")),
        ("*", Intrinsic::DoubleBin("*")),
        ("/", Intrinsic::DoubleBin("/")),
        ("%", Intrinsic::DoubleBin("%")),
    ] {
        method(st, c, op, vec![Type::Double], Type::Double, ic);
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Double],
            Type::Boolean,
            Intrinsic::DoubleBin(op),
        );
    }
    method(
        st,
        c,
        "unary_-",
        vec![],
        Type::Double,
        Intrinsic::DoubleUn("-"),
    );
}

fn add_float_members(st: &mut SymbolTable) {
    let c = st.float_sym;
    for op in ["+", "-", "*", "/", "%"] {
        method(
            st,
            c,
            op,
            vec![Type::Float],
            Type::Float,
            Intrinsic::FloatBin(op),
        );
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Float],
            Type::Boolean,
            Intrinsic::FloatBin(op),
        );
    }
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::FloatToDouble,
    );
    method(
        st,
        c,
        "unary_-",
        vec![],
        Type::Float,
        Intrinsic::FloatUn("-"),
    );
}

fn add_bool_members(st: &mut SymbolTable) {
    let c = st.boolean_sym;
    method(
        st,
        c,
        "&&",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("&&"),
    );
    method(
        st,
        c,
        "||",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("||"),
    );
    method(
        st,
        c,
        "unary_!",
        vec![],
        Type::Boolean,
        Intrinsic::BoolUn("!"),
    );
    method(
        st,
        c,
        "==",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("=="),
    );
    method(
        st,
        c,
        "!=",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("!="),
    );
}

fn add_string_members(st: &mut SymbolTable, library_abi: bool) {
    let c = st.string_sym;
    method(
        st,
        c,
        "+",
        vec![Type::Any],
        Type::String,
        Intrinsic::StringConcat,
    );
    method(
        st,
        c,
        "charAt",
        vec![Type::Int],
        Type::Char,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "concat",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    if !library_abi {
        method(st, c, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    }
    method(
        st,
        c,
        "equals",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, c, "toString", vec![], Type::String, Intrinsic::Identity);
    // nsc calls java.lang.String for these; StringOps has no $extension.
    method(
        st,
        c,
        "startsWith",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "endsWith",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "indexOf",
        vec![Type::String],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "split",
        vec![Type::String],
        Type::Array(Box::new(Type::String)),
        Intrinsic::None,
    );
    if !library_abi {
        method(st, c, "length", vec![], Type::Int, Intrinsic::None);
        // Private runtime: parseInt on String. Library mode uses StringOps via augmentString.
        method(st, c, "toInt", vec![], Type::Int, Intrinsic::StringToInt);
        method(st, c, "toLong", vec![], Type::Long, Intrinsic::StringToLong);
        method(
            st,
            c,
            "toDouble",
            vec![],
            Type::Double,
            Intrinsic::StringToDouble,
        );
    }
}

fn add_array_members(st: &mut SymbolTable) {
    let c = st.array_sym;
    method(st, c, "length", vec![], Type::Int, Intrinsic::None);
    method(st, c, "apply", vec![Type::Int], Type::Any, Intrinsic::None);
    method(
        st,
        c,
        "update",
        vec![Type::Int, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
}

pub(crate) fn fn1(arg: Type, ret: Type) -> Type {
    Type::Function {
        params: vec![arg],
        ret: Box::new(ret),
    }
}

pub(crate) fn fn2(a: Type, b: Type, ret: Type) -> Type {
    Type::Function {
        params: vec![a, b],
        ret: Box::new(ret),
    }
}

pub(crate) fn fn_n(params: Vec<Type>, ret: Type) -> Type {
    Type::Function {
        params,
        ret: Box::new(ret),
    }
}

/// `class WithFilter[+A, +CC[_]]`, as 2.13 declares it.
///
/// `CC` is a type *constructor*: `map[B](f: A => B): CC[B]` is what makes
/// `for (x <- xs if p) yield x.toString` a `List[String]`. Holding the
/// filtered collection whole (`CC = List[A]`, `map: CC`) made every guarded
/// comprehension keep the element type it started with.
fn add_with_filter(st: &mut SymbolTable) -> SymbolId {
    let wf = class(
        st,
        st.scala_pkg,
        "WithFilter",
        "scala/collection/WithFilter",
        &[Type::AnyRef],
    );
    let a = type_param(st, wf, "A");
    let cc = type_param(st, wf, "CC");
    let cc_x = type_param(st, cc, "X");
    st.get_mut(cc).tparams = vec![cc_x];
    st.get_mut(wf).tparams = vec![a, cc];
    let ta = Type::TypeParam(a);
    let tcc = Type::TypeParam(cc);
    let applied = |arg: Type| Type::Applied {
        ctor: Box::new(tcc.clone()),
        args: vec![arg],
    };
    let m = method(
        st,
        wf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Any,
        Intrinsic::None,
    );
    let b = type_param(st, m, "B");
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(b))]],
        ret: Box::new(applied(Type::TypeParam(b))),
    };
    let fm = method(
        st,
        wf,
        "flatMap",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Any,
        Intrinsic::None,
    );
    let fb = type_param(st, fm, "B");
    st.get_mut(fm).tparams = vec![fb];
    st.get_mut(fm).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), applied(Type::TypeParam(fb)))]],
        ret: Box::new(applied(Type::TypeParam(fb))),
    };
    method(
        st,
        wf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        Type::Class {
            sym: wf,
            args: vec![Type::TypeParam(a), tcc],
        },
        Intrinsic::None,
    );
    wf
}

fn add_option_with_filter(st: &mut SymbolTable) -> SymbolId {
    let wf = class(
        st,
        st.scala_pkg,
        "Option$WithFilter",
        "scala/Option$WithFilter",
        &[Type::AnyRef],
    );
    let a = type_param(st, wf, "A");
    st.get_mut(wf).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let opt = Type::Class {
        sym: st.option_sym,
        args: vec![ta.clone()],
    };
    // `def map[B](f: A => B): Option[B]` -- the element type is what `f`
    // returns, not what the filter was applied to.
    let m = method(
        st,
        wf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Any,
        Intrinsic::None,
    );
    let mb = type_param(st, m, "B");
    st.get_mut(m).tparams = vec![mb];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(mb))]],
        ret: Box::new(Type::Class {
            sym: st.option_sym,
            args: vec![Type::TypeParam(mb)],
        }),
    };
    let fm = method(
        st,
        wf,
        "flatMap",
        vec![fn1(ta.clone(), opt.clone())],
        Type::Any,
        Intrinsic::None,
    );
    let fb = type_param(st, fm, "B");
    st.get_mut(fm).tparams = vec![fb];
    let opt_b = Type::Class {
        sym: st.option_sym,
        args: vec![Type::TypeParam(fb)],
    };
    st.get_mut(fm).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), opt_b.clone())]],
        ret: Box::new(opt_b),
    };
    let _ = opt;
    method(
        st,
        wf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        Type::Class {
            sym: wf,
            args: vec![Type::TypeParam(a)],
        },
        Intrinsic::None,
    );
    wf
}

fn add_iterator(st: &mut SymbolTable) -> SymbolId {
    let it = iface(st, st.scala_pkg, "Iterator", "scala/collection/Iterator");
    let a = type_param(st, it, "A");
    st.get_mut(it).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let it_t = Type::Class {
        sym: it,
        args: vec![ta.clone()],
    };
    method(st, it, "hasNext", vec![], Type::Boolean, Intrinsic::None);
    method(st, it, "next", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        it,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        it,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        it_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        it,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        it_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        it,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        it_t,
        Intrinsic::None,
    );
    it
}

fn add_string_ops(st: &mut SymbolTable, iterator: SymbolId) -> SymbolId {
    let so = class(
        st,
        st.scala_pkg,
        "StringOps",
        "scala/collection/StringOps",
        &[Type::AnyVal],
    );
    let f = st.alloc("repr", so, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::String;
    st.get_mut(so).ctor_fields = vec![f];
    method(st, so, "toInt", vec![], Type::Int, Intrinsic::None);
    method(st, so, "toLong", vec![], Type::Long, Intrinsic::None);
    method(st, so, "toDouble", vec![], Type::Double, Intrinsic::None);
    method(st, so, "length", vec![], Type::Int, Intrinsic::None);
    method(st, so, "size", vec![], Type::Int, Intrinsic::None);
    method(st, so, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, so, "*", vec![Type::Int], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "take",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "drop",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "toUpperCase", vec![], Type::String, Intrinsic::None);
    method(st, so, "toLowerCase", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "stripPrefix",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "split",
        vec![Type::Char],
        Type::Array(Box::new(Type::String)),
        Intrinsic::None,
    );
    method(
        st,
        so,
        "stripSuffix",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "padTo",
        vec![Type::Int, Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "linesIterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toIntOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Int],
        },
        Intrinsic::None,
    );
    method(st, so, "stripMargin", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "stripMargin",
        vec![Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "lines",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
    method(st, so, "capitalize", vec![], Type::String, Intrinsic::None);
    method(st, so, "reverse", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "slice",
        vec![Type::Int, Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "takeRight",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "dropRight",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "contains",
        vec![Type::Char],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, so, "head", vec![], Type::Char, Intrinsic::None);
    method(st, so, "last", vec![], Type::Char, Intrinsic::None);
    method(
        st,
        so,
        "stripLineEnd",
        vec![],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "replaceAllLiterally",
        vec![Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "tail", vec![], Type::String, Intrinsic::None);
    method(st, so, "init", vec![], Type::String, Intrinsic::None);
    method(st, so, "distinct", vec![], Type::String, Intrinsic::None);
    method(st, so, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "filter",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "reverseIterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::Char],
        },
        Intrinsic::None,
    );
    let seq = crate::classpath::find_or_stub_java_class(st, "scala/collection/Seq");
    // `Seq` carries a type parameter (`prelude_hier` gives the stub one), so
    // name the element type instead of leaving the parameter raw.
    let seq_char = Type::Class {
        sym: seq,
        args: vec![Type::Char],
    };
    method(
        st,
        so,
        "diff",
        vec![seq_char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "intersect",
        vec![Type::Class {
            sym: seq,
            args: vec![],
        }],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "updated",
        vec![Type::Int, Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "count",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "exists",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "forall",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "copyToArray",
        vec![Type::Array(Box::new(Type::Char))],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "copyToArray",
        vec![Type::Array(Box::new(Type::Char)), Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "copyToArray",
        vec![Type::Array(Box::new(Type::Char)), Type::Int, Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(st, so, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        so,
        "takeWhile",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "dropWhile",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "filterNot",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    let opt_char = Type::Class {
        sym: st.option_sym,
        args: vec![Type::Char],
    };
    method(
        st,
        so,
        "headOption",
        vec![],
        opt_char.clone(),
        Intrinsic::None,
    );
    method(
        st,
        so,
        "lastOption",
        vec![],
        opt_char.clone(),
        Intrinsic::None,
    );
    method(
        st,
        so,
        "find",
        vec![fn1(Type::Char, Type::Boolean)],
        opt_char,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "foreach",
        vec![fn1(Type::Char, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, so, "toBoolean", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        so,
        "toBooleanOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Boolean],
        },
        Intrinsic::None,
    );
    method(st, so, "toByte", vec![], Type::Byte, Intrinsic::None);
    method(st, so, "toShort", vec![], Type::Short, Intrinsic::None);
    method(st, so, "toFloat", vec![], Type::Float, Intrinsic::None);
    method(
        st,
        so,
        "toByteOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Byte],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toShortOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Short],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toFloatOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Float],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toLongOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Long],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toDoubleOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Double],
        },
        Intrinsic::None,
    );
    so
}

fn add_array_ops(st: &mut SymbolTable) -> SymbolId {
    let aops = class(
        st,
        st.scala_pkg,
        "ArrayOps",
        "scala/collection/ArrayOps",
        &[Type::AnyVal],
    );
    let a = type_param(st, aops, "A");
    st.get_mut(aops).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let xs = st.alloc("xs", aops, SymKind::Term, Flags::PARAM, "");
    st.get_mut(xs).ty = Type::Array(Box::new(ta.clone()));
    st.get_mut(aops).ctor_fields = vec![xs];
    method(st, aops, "head", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        aops,
        "tail",
        vec![],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "take",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "drop",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "dropWhile",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "exists",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "count",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "forall",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "slice",
        vec![Type::Int, Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(st, aops, "last", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        aops,
        "init",
        vec![],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "reverse",
        vec![],
        Type::Array(Box::new(ta)),
        Intrinsic::None,
    );
    method(st, aops, "size", vec![], Type::Int, Intrinsic::None);
    method(st, aops, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, aops, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    aops
}

/// `ArrayOps.map[B](f: A => B)(implicit ClassTag[B]): Array[B]` (scala-library 2.13).
fn add_array_ops_map(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "map", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(b));
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta, Type::TypeParam(b))],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(b)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}

/// `ArrayOps.flatMap[B](f: A => Any)(implicit ClassTag[B]): Array[B]`.
/// Dual-run uses `List` (IterableOnce); the Array→Iterable 4-arg overload is
/// `add_array_ops_flat_map_from_array`.
fn add_array_ops_flat_map(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "flatMap", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::Any);
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta, Type::Any)],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(b)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}

/// `ArrayOps.flatMap[BS, B](f: A => BS)(implicit asIterable: BS => Iterable[B], m: ClassTag[B])`.
/// nsc 2.13.16 4-arg JVM: `flatMap$extension(Object, Function1, Function1, ClassTag)Object`.
fn add_array_ops_flat_map_from_array(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let iterable = iface(st, coll, "Iterable", "scala/collection/Iterable");
    if st.get(iterable).tparams.is_empty() {
        let ia = type_param(st, iterable, "A");
        st.get_mut(iterable).tparams = vec![ia];
    }
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let _of_int = class(
        st,
        mutp,
        "ArraySeq$ofInt",
        "scala/collection/mutable/ArraySeq$ofInt",
        &[Type::Class {
            sym: iterable,
            args: vec![Type::Int],
        }],
    );
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "flatMap", vec![], Type::Unit, Intrinsic::None);
    let bs = type_param(st, m, "BS");
    let b = type_param(st, m, "B");
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(bs));
    let as_it = st.alloc(
        "asIterable",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(as_it).ty = fn1(
        Type::TypeParam(bs),
        Type::Class {
            sym: iterable,
            args: vec![Type::TypeParam(b)],
        },
    );
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![bs, b];
    st.get_mut(m).params = vec![f, as_it, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![as_it, ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta, Type::TypeParam(bs))],
            vec![
                fn1(
                    Type::TypeParam(bs),
                    Type::Class {
                        sym: iterable,
                        args: vec![Type::TypeParam(b)],
                    },
                ),
                Type::Class {
                    sym: ct,
                    args: vec![Type::TypeParam(b)],
                },
            ],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}

/// `ArrayOps.collect[B](pf: PartialFunction[A, B])(implicit ClassTag[B]): Array[B]`.
/// nsc 2.13.16 JVM: `collect$extension(Object, PartialFunction, ClassTag)Object`.
fn add_array_ops_collect(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let pf = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "PartialFunction")
        .unwrap_or(SymbolId::NONE);
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "collect", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let f = st.alloc("pf", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::Class {
        sym: pf,
        args: vec![ta.clone(), Type::TypeParam(b)],
    };
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![Type::Class {
                sym: pf,
                args: vec![ta, Type::TypeParam(b)],
            }],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(b)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}

/// `ArrayOps.zip[B](that: IterableOnce[B]): Array[(A, B)]`.
/// nsc 2.13.16 JVM: `zip$extension(Object, IterableOnce)Tuple2[]`.
fn add_array_ops_zip(st: &mut SymbolTable, aops: SymbolId, tuple2: SymbolId) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let ioc = iface(st, coll, "IterableOnce", "scala/collection/IterableOnce");
    if st.get(ioc).tparams.is_empty() {
        let ia = type_param(st, ioc, "A");
        st.get_mut(ioc).tparams = vec![ia];
    }
    if let Some(la) = st.get(st.list_sym).tparams.first().copied() {
        let parent = Type::Class {
            sym: ioc,
            args: vec![Type::TypeParam(la)],
        };
        if !st
            .get(st.list_sym)
            .parents
            .iter()
            .any(|p| matches!(p, Type::Class { sym, .. } if *sym == ioc))
        {
            st.get_mut(st.list_sym).parents.push(parent);
        }
    }
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "zip", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let that = st.alloc("that", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(that).ty = Type::Class {
        sym: ioc,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![that];
    st.get_mut(m).paramss = vec![vec![that]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ioc,
            args: vec![Type::TypeParam(b)],
        }]],
        ret: Box::new(Type::Array(Box::new(Type::Class {
            sym: tuple2,
            args: vec![ta, Type::TypeParam(b)],
        }))),
    };
}

/// ArrayOps.foldLeft / fold / foldRight.
///
/// nsc 2.13.16 JVM: `foldLeft$extension(Object, Object, Function2)Object` 系。
/// `reduce` は ArrayOps に無いので載せない。
fn add_array_ops_folds(st: &mut SymbolTable, aops: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);

    let m = method(st, aops, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), ta.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), ta.clone(), tb.clone())],
        ],
        ret: Box::new(tb),
    };

    let m = method(st, aops, "fold", vec![], Type::Unit, Intrinsic::None);
    let a1 = type_param(st, m, "A1");
    let ta1 = Type::TypeParam(a1);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = ta1.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(ta1.clone(), ta1.clone(), ta1.clone());
    st.get_mut(m).tparams = vec![a1];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![ta1.clone()],
            vec![fn2(ta1.clone(), ta1.clone(), ta1.clone())],
        ],
        ret: Box::new(ta1),
    };

    let m = method(st, aops, "foldRight", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(ta.clone(), tb.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![tb.clone()], vec![fn2(ta, tb.clone(), tb.clone())]],
        ret: Box::new(tb),
    };
}

/// ArrayOps.scanLeft[B: ClassTag](z: B)(op: (B, A) => B): Array[B]
///
/// nsc 2.13.16 JVM: `scanLeft$extension(Object, Object, Function2, ClassTag)Object`.
fn add_array_ops_scan_left(st: &mut SymbolTable, aops: SymbolId) {
    let reflect = crate::classpath::ensure_package(st, "scala/reflect");
    let ct = st
        .lookup_member(reflect, "ClassTag")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "scanLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), ta.clone(), tb.clone());
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![tb.clone()],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op, ev];
    st.get_mut(m).paramss = vec![vec![z], vec![op], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), ta, tb.clone())],
            vec![Type::Class {
                sym: ct,
                args: vec![tb.clone()],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(tb))),
    };
}

/// StringOps.foldLeft[B](z: B)(op: (B, Char) => B): B
///
/// nsc 2.13.16 JVM: `foldLeft$extension(String, Object, Function2)Object`.
fn add_string_ops_fold_left(st: &mut SymbolTable, so: SymbolId) {
    let m = method(st, so, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), Type::Char, tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), Type::Char, tb.clone())],
        ],
        ret: Box::new(tb),
    };
}

/// StringOps.foldRight[B](z: B)(op: (Char, B) => B): B and grouped(n): Iterator[String].
///
/// nsc 2.13.16 JVM: `foldRight$extension(String, Object, Function2)Object` /
/// `grouped$extension(String, I)Iterator`.
fn add_string_ops_fold_right_and_grouped(st: &mut SymbolTable, so: SymbolId) {
    let m = method(st, so, "foldRight", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(Type::Char, tb.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(Type::Char, tb.clone(), tb.clone())],
        ],
        ret: Box::new(tb),
    };
    let it = st
        .lookup_member(st.scala_pkg, "Iterator")
        .into_iter()
        .find(|&id| {
            matches!(
                st.get(id).kind,
                crate::symbol::SymKind::Class | crate::symbol::SymKind::ModuleClass
            )
        })
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        so,
        "grouped",
        vec![Type::Int],
        Type::Class {
            sym: it,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
}

/// ArrayOps.find / contains / distinct / takeRight / dropRight / takeWhile /
/// indices / lengthCompare against 2.13.16.
///
/// JVM: `find$extension(Object, Function1)Option`,
/// `contains$extension(Object, Object)Z`, `distinct$extension(Object)Object`,
/// `takeRight$extension` / `dropRight$extension` `(Object, I)Object`,
/// `takeWhile$extension(Object, Function1)Object`,
/// `indices$extension(Object)Range`, `lengthCompare$extension(Object, I)I`.
fn add_array_ops_remaining(st: &mut SymbolTable, aops: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    method(
        st,
        aops,
        "find",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Class {
            sym: st.option_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "contains",
        vec![ta.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "distinct",
        vec![],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "takeRight",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "dropRight",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "takeWhile",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "lengthCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    let range = st
        .lookup_member(st.scala_pkg, "Range")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        aops,
        "indices",
        vec![],
        Type::Class {
            sym: range,
            args: vec![],
        },
        Intrinsic::None,
    );
}

/// ArrayOps.filterNot / headOption / lastOption / partition / splitAt / span
/// against 2.13.16.
///
/// JVM: `filterNot$extension(Object, Function1)Object`,
/// `headOption$extension` / `lastOption$extension(Object)Option`,
/// `partition$extension` / `span$extension(Object, Function1)Tuple2`,
/// `splitAt$extension(Object, I)Tuple2`.
fn add_array_ops_filter_not_opts_part(st: &mut SymbolTable, aops: SymbolId, tuple2: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let arr = Type::Array(Box::new(ta.clone()));
    method(
        st,
        aops,
        "filterNot",
        vec![fn1(ta.clone(), Type::Boolean)],
        arr.clone(),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "headOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "lastOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    let pair = Type::Class {
        sym: tuple2,
        args: vec![arr.clone(), arr],
    };
    method(
        st,
        aops,
        "partition",
        vec![fn1(ta.clone(), Type::Boolean)],
        pair.clone(),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "splitAt",
        vec![Type::Int],
        pair.clone(),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "span",
        vec![fn1(ta, Type::Boolean)],
        pair,
        Intrinsic::None,
    );
}

/// ArrayOps.zipWithIndex / knownSize / sizeCompare against 2.13.16.
///
/// JVM: `zipWithIndex$extension(Object)[Lscala/Tuple2;`,
/// `knownSize$extension(Object)I`, `sizeCompare$extension(Object, I)I`.
fn add_array_ops_zip_index_size(st: &mut SymbolTable, aops: SymbolId, tuple2: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    method(
        st,
        aops,
        "zipWithIndex",
        vec![],
        Type::Array(Box::new(Type::Class {
            sym: tuple2,
            args: vec![ta, Type::Int],
        })),
        Intrinsic::None,
    );
    method(st, aops, "knownSize", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        aops,
        "sizeCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
}

/// ArrayOps.lengthIs / sizeIs / indexOf / copyToArray / iterator against 2.13.16.
///
/// JVM: `lengthIs$extension` / `sizeIs$extension(Object)I`,
/// `indexOf$extension(Object, Object, I)I`,
/// `copyToArray$extension(Object, Object)I`,
/// `iterator$extension(Object)Iterator`.
fn add_array_ops_length_index_copy(st: &mut SymbolTable, aops: SymbolId, iterator: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    method(st, aops, "lengthIs", vec![], Type::Int, Intrinsic::None);
    method(st, aops, "sizeIs", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        aops,
        "indexOf",
        vec![ta.clone(), Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "copyToArray",
        vec![Type::Array(Box::new(ta.clone()))],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![ta],
        },
        Intrinsic::None,
    );
}

/// StringOps.map(Char => Char): String, `:+` / `+:` against 2.13.16.
///
/// JVM: `map$extension(String, Function1)String`,
/// `$colon$plus$extension(String, C)String`, `$plus$colon$extension(String, C)String`.
fn add_string_ops_map_and_appended(st: &mut SymbolTable, so: SymbolId) {
    method(
        st,
        so,
        "map",
        vec![fn1(Type::Char, Type::Char)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        ":+",
        vec![Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "+:",
        vec![Type::Char],
        Type::String,
        Intrinsic::None,
    );
}

/// StringOps.compare / lengthCompare / patch(Int, String, Int) / `<` / `>` /
/// `>=` / `<=` against 2.13.16.
///
/// JVM: `compare$extension(String, String)I`, `lengthCompare$extension(String, I)I`,
/// `patch$extension(String, I, String, I)String`, `$less$extension` /
/// `$greater$extension` / `$greater$eq$extension` / `$less$eq$extension`
/// `(String, String)Z`.
fn add_string_ops_compare_patch_length(st: &mut SymbolTable, so: SymbolId) {
    method(
        st,
        so,
        "compare",
        vec![Type::String],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "lengthCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "patch",
        vec![Type::Int, Type::String, Type::Int],
        Type::String,
        Intrinsic::None,
    );
    for op in ["<", ">", ">=", "<="] {
        method(
            st,
            so,
            op,
            vec![Type::String],
            Type::Boolean,
            Intrinsic::None,
        );
    }
}

/// StringOps.iterator / sizeCompare / knownSize / appendedAll / prependedAll
/// against 2.13.16.
///
/// JVM: `iterator$extension(String)Iterator`, `sizeCompare$extension(String, I)I`,
/// `knownSize$extension(String)I`, `appendedAll$extension` /
/// `prependedAll$extension(String, String)String`.
fn add_string_ops_iterator_size_appended(st: &mut SymbolTable, so: SymbolId, iterator: SymbolId) {
    method(
        st,
        so,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::Char],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "sizeCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(st, so, "knownSize", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        so,
        "appendedAll",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "prependedAll",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
}

/// StringOps.`++` / lengthIs / sizeIs / flatMap(Char => String) against 2.13.16.
///
/// JVM: `$plus$plus$extension(String, String)String`,
/// `lengthIs$extension` / `sizeIs$extension(String)I`,
/// `flatMap$extension(String, Function1)String`.
fn add_string_ops_concat_length_flat(st: &mut SymbolTable, so: SymbolId) {
    method(
        st,
        so,
        "++",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "lengthIs", vec![], Type::Int, Intrinsic::None);
    method(st, so, "sizeIs", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        so,
        "flatMap",
        vec![fn1(Type::Char, Type::String)],
        Type::String,
        Intrinsic::None,
    );
}

fn add_string_ops_indices_and_r(st: &mut SymbolTable, so: SymbolId) {
    let range = st
        .lookup_member(st.scala_pkg, "Range")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        so,
        "indices",
        vec![],
        Type::Class {
            sym: range,
            args: vec![],
        },
        Intrinsic::None,
    );
    let matching = crate::classpath::ensure_package(st, "scala/util/matching");
    let regex = class(
        st,
        matching,
        "Regex",
        "scala/util/matching/Regex",
        &[Type::AnyRef],
    );
    method(
        st,
        regex,
        "findFirstIn",
        vec![Type::String],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
    method(
        st,
        regex,
        "matches",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "r",
        vec![],
        Type::Class {
            sym: regex,
            args: vec![],
        },
        Intrinsic::None,
    );
}

fn add_bit_set(st: &mut SymbolTable) {
    let immp = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let bs = class(
        st,
        immp,
        "BitSet",
        "scala/collection/immutable/BitSet",
        &[Type::AnyRef],
    );
    method(
        st,
        bs,
        "contains",
        vec![Type::Int],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        bs,
        "foreach",
        vec![fn1(Type::Int, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let bs_t = Type::Class {
        sym: bs,
        args: vec![],
    };
    let bs_mod = module(st, immp, "BitSet", "scala/collection/immutable/BitSet$");
    let bs_cls = st.module_class_of(bs_mod);
    method(
        st,
        bs_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Int))],
        bs_t,
        Intrinsic::None,
    );
    let mems = st.get(bs_cls).members.clone();
    st.get_mut(bs_mod).members.extend(mems);
}

/// `StringOps.toArray` with `ClassTag[Char]` — nsc `toArray[B >: Char : ClassTag]`.
fn add_string_ops_to_array(st: &mut SymbolTable, so: SymbolId, ct: SymbolId) {
    let m = method(
        st,
        so,
        "toArray",
        vec![],
        Type::Array(Box::new(Type::Char)),
        Intrinsic::None,
    );
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::Char],
    };
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ct,
            args: vec![Type::Char],
        }]],
        ret: Box::new(Type::Array(Box::new(Type::Char))),
    };
}

/// `StringOps.sorted` with implicit `Ordering[Char]` (`Ordering$Char$.MODULE$`).
fn add_string_ops_sorted(st: &mut SymbolTable, so: SymbolId, ordering: SymbolId) {
    let m = method(st, so, "sorted", vec![], Type::String, Intrinsic::None);
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::Char],
    };
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ordering,
            args: vec![Type::Char],
        }]],
        ret: Box::new(Type::String),
    };
}

fn add_option_members(st: &mut SymbolTable, option_wf: SymbolId, library_abi: bool) {
    let o = st.option_sym;
    let a = type_param(st, o, "A");
    st.get_mut(o).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let opt = Type::Class {
        sym: o,
        args: vec![ta.clone()],
    };
    method(st, o, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, o, "get", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        o,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        o,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "flatMap",
        vec![fn1(ta.clone(), opt.clone())],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "withFilter",
        vec![fn1(ta.clone(), Type::Boolean)],
        if library_abi {
            Type::Class {
                sym: option_wf,
                args: vec![ta],
            }
        } else {
            opt
        },
        Intrinsic::None,
    );

    let some = st.some_sym;
    let sa = type_param(st, some, "A");
    st.get_mut(some).tparams = vec![sa];
    let tsa = Type::TypeParam(sa);
    // `Some[A] extends Option[A]`: without the argument a `case Some(x)` on an
    // `Option[Int]` cannot recover `Int`.
    st.get_mut(some).parents = vec![Type::Class {
        sym: o,
        args: vec![tsa.clone()],
    }];
    method(
        st,
        some,
        "<init>",
        vec![tsa.clone()],
        Type::Class {
            sym: some,
            args: vec![tsa.clone()],
        },
        Intrinsic::None,
    );
    st.get_mut(some).ctor_fields = {
        // The jar's `Some.value` field is private; destructuring goes through
        // the `value()` accessor. The private runtime keeps a public field.
        let acc = if library_abi { "value" } else { "" };
        let f = st.alloc("value", some, SymKind::Term, Flags::PARAM, acc);
        st.get_mut(f).ty = tsa;
        vec![f]
    };
    // `st.none_sym` is the *module* symbol; its expression type is
    // `Type::ModuleRef(<module class>)` (see `prelude::module`), so anything
    // that walks `None`'s ancestry from a typed `None` expression (e.g.
    // `SymbolTable::lub`'s `base_type_seq`) reads the *module class*'s
    // `parents`, not the module's own. Setting `.parents` on `none_sym` here
    // was a no-op for that purpose: `module_extending` (which created
    // `none_sym`) had already stamped the module *class* with the raw,
    // unparameterized `Option` from its `parent` argument, and this line
    // never touched that copy. The result: `lub(None, Some(x))` degraded to
    // raw `Option` (dropping the element type) instead of `Option[X]`,
    // e.g. `val r = if (c) None else Some(x)` losing `x`'s type. Fixed by
    // writing to the module class, matching `module_extending`.
    let none_cls = st.module_class_of(st.none_sym);
    st.get_mut(none_cls).parents = vec![Type::Class {
        sym: o,
        args: vec![Type::Nothing],
    }];
}

fn add_cons_members(st: &mut SymbolTable, library_abi: bool) {
    let cons = st.cons_sym;
    let ca = type_param(st, cons, "A");
    st.get_mut(cons).tparams = vec![ca];
    let tca = Type::TypeParam(ca);
    let list_ca = Type::Class {
        sym: st.list_sym,
        args: vec![tca.clone()],
    };
    // `::[A] extends List[A]`, so `case h :: t` on a `List[Int]` binds `h: Int`.
    st.get_mut(cons).parents = vec![list_ca.clone()];
    // `Nil` is built before `List` has its type parameter, so the parent it
    // was given then is the *raw* `List`. Restate it now that `List[A]`
    // exists — and on the module *class*, which is what `Type::ModuleRef`
    // names and where the parent walk looks; the module symbol's own parent
    // list is never consulted.
    let nil_cls = st.module_class_of(st.nil_sym);
    let nil_parent = vec![Type::Class {
        sym: st.list_sym,
        args: vec![Type::Nothing],
    }];
    st.get_mut(st.nil_sym).parents = nil_parent.clone();
    st.get_mut(nil_cls).parents = nil_parent;
    let (head_acc, tail_acc) = if library_abi {
        ("head", "tail")
    } else {
        ("", "")
    };
    let h = st.alloc("head", cons, SymKind::Term, Flags::PARAM, head_acc);
    st.get_mut(h).ty = tca;
    let t = st.alloc("tl", cons, SymKind::Term, Flags::PARAM, tail_acc);
    st.get_mut(t).ty = list_ca;
    st.get_mut(cons).ctor_fields = vec![h, t];
    let f = st.get(cons).flags.with(Flags::CASE);
    st.get_mut(cons).flags = f;
}

fn add_list_members(
    st: &mut SymbolTable,
    with_filter: SymbolId,
    iterator: Option<SymbolId>,
    library_abi: bool,
) {
    let l = st.list_sym;
    let a = type_param(st, l, "A");
    st.get_mut(l).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let list_t = Type::Class {
        sym: l,
        args: vec![ta.clone()],
    };
    method(st, l, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, l, "head", vec![], ta.clone(), Intrinsic::None);
    method(st, l, "tail", vec![], list_t.clone(), Intrinsic::None);
    method(
        st,
        l,
        "::",
        vec![Type::Any],
        list_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        l,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        l,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        list_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        l,
        "flatMap",
        vec![fn1(ta.clone(), list_t.clone())],
        list_t.clone(),
        Intrinsic::None,
    );
    let wf_ret = if library_abi {
        Type::Class {
            sym: with_filter,
            // `CC` is the *constructor* `List`, so `map[B]` gives `List[B]`.
            args: vec![
                ta.clone(),
                Type::Class {
                    sym: l,
                    args: vec![],
                },
            ],
        }
    } else {
        list_t.clone()
    };
    method(
        st,
        l,
        "withFilter",
        vec![fn1(ta.clone(), Type::Boolean)],
        wf_ret,
        Intrinsic::None,
    );
    if let Some(it) = iterator {
        method(
            st,
            l,
            "iterator",
            vec![],
            Type::Class {
                sym: it,
                args: vec![ta.clone()],
            },
            Intrinsic::None,
        );
    }

    let list_mod = module(st, st.scala_pkg, "List", "scala/collection/immutable/List$");
    let mcls = st.module_class_of(list_mod);
    let seq = method(
        st,
        mcls,
        "unapplySeq",
        vec![list_t.clone()],
        Type::Class {
            sym: st.option_sym,
            args: vec![list_t.clone()],
        },
        Intrinsic::None,
    );
    let _ = seq;
    if library_abi {
        let list_apply = method(
            st,
            mcls,
            "apply",
            vec![Type::Repeated(Box::new(Type::Any))],
            list_t.clone(),
            Intrinsic::None,
        );
        let la = type_param(st, list_apply, "A");
        st.get_mut(list_apply).tparams = vec![la];
        st.get_mut(list_apply).ty = Type::Method {
            paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(la)))]],
            ret: Box::new(Type::Class {
                sym: l,
                args: vec![Type::TypeParam(la)],
            }),
        };
    }
    let mems = st.get(mcls).members.clone();
    st.get_mut(list_mod).members.extend(mems);
}

fn add_function_types(st: &mut SymbolTable) {
    // Function3/4 are needed for `Using.resources` 3–4 resource overloads.
    for n in 0..=4 {
        let f = iface(
            st,
            st.scala_pkg,
            &format!("Function{n}"),
            &format!("scala/Function{n}"),
        );
        // `FunctionN` really is `FunctionN[-T1, …, -Tn, +R]`, and `apply` is
        // its one *abstract* method. Both matter: a trait written as
        // `trait C[-T] extends (T => R)` inherits that `apply`, and reading it
        // through `C[X]` is what makes `C` a SAM whose parameter is `X`.
        // Without the parameters there is nothing for `subst_as_seen_from` to
        // substitute, and without `ABSTRACT` the SAM search finds no method.
        let mut tps: Vec<SymbolId> = (1..=n)
            .map(|i| type_param(st, f, &format!("T{i}")))
            .collect();
        let r = type_param(st, f, "R");
        tps.push(r);
        st.get_mut(f).tparams = tps.clone();
        let params: Vec<Type> = tps[..n].iter().map(|p| Type::TypeParam(*p)).collect();
        let apply = method(st, f, "apply", params, Type::TypeParam(r), Intrinsic::None);
        st.get_mut(apply).flags = Flags::ABSTRACT;
    }
}

fn add_partial_function(st: &mut SymbolTable) {
    let f1 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Function1")
        .unwrap_or(SymbolId::NONE);
    let pf = iface(st, st.scala_pkg, "PartialFunction", "scala/PartialFunction");
    let a = type_param(st, pf, "A");
    let b = type_param(st, pf, "B");
    st.get_mut(pf).tparams = vec![a, b];
    let ta = Type::TypeParam(a);
    let tb = Type::TypeParam(b);
    st.get_mut(pf).parents = vec![
        Type::Class {
            sym: f1,
            args: vec![ta.clone(), tb.clone()],
        },
        Type::AnyRef,
    ];
    method(
        st,
        pf,
        "apply",
        vec![ta.clone()],
        tb.clone(),
        Intrinsic::None,
    );
    method(
        st,
        pf,
        "isDefinedAt",
        vec![ta.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        pf,
        "applyOrElse",
        vec![ta.clone(), fn1(ta, tb.clone())],
        tb,
        Intrinsic::None,
    );
}

fn add_list_collect(st: &mut SymbolTable) {
    let pf = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "PartialFunction")
        .unwrap_or(SymbolId::NONE);
    let l = st.list_sym;
    let a = st.get(l).tparams.first().copied().unwrap_or(SymbolId::NONE);
    let ta = if a.is_none() {
        Type::Any
    } else {
        Type::TypeParam(a)
    };
    let list_t = Type::Class {
        sym: l,
        args: vec![ta.clone()],
    };
    let pf_ty = Type::Class {
        sym: pf,
        args: vec![ta, Type::Any],
    };
    method(st, l, "collect", vec![pf_ty], list_t, Intrinsic::None);
}

fn add_map_and_vector(st: &mut SymbolTable) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);

    let map = iface(st, st.scala_pkg, "Map", "scala/collection/immutable/Map");
    let mk = type_param(st, map, "K");
    let mv = type_param(st, map, "V");
    st.get_mut(map).tparams = vec![mk, mv];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let map_t = Type::Class {
        sym: map,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };
    method(
        st,
        map,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "get",
        vec![Type::Any],
        Type::Class {
            sym: st.option_sym,
            args: vec![tv.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "updated",
        vec![Type::Any, Type::Any],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "+",
        vec![pair.clone()],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let map_mod = module(st, st.scala_pkg, "Map", "scala/collection/immutable/Map$");
    let map_cls = st.module_class_of(map_mod);
    method(
        st,
        map_cls,
        "empty",
        vec![],
        Type::Class {
            sym: map,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let map_apply = method(
        st,
        map_cls,
        "apply",
        vec![Type::Repeated(Box::new(pair.clone()))],
        map_t.clone(),
        Intrinsic::None,
    );
    let mak = type_param(st, map_apply, "K");
    let mav = type_param(st, map_apply, "V");
    st.get_mut(map_apply).tparams = vec![mak, mav];
    let map_pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(mak), Type::TypeParam(mav)],
    };
    st.get_mut(map_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(map_pair))]],
        ret: Box::new(Type::Class {
            sym: map,
            args: vec![Type::TypeParam(mak), Type::TypeParam(mav)],
        }),
    };
    let mems = st.get(map_cls).members.clone();
    st.get_mut(map_mod).members.extend(mems);

    let vec = class(
        st,
        st.scala_pkg,
        "Vector",
        "scala/collection/immutable/Vector",
        &[Type::AnyRef],
    );
    let va = type_param(st, vec, "A");
    st.get_mut(vec).tparams = vec![va];
    let ta = Type::TypeParam(va);
    let vec_t = Type::Class {
        sym: vec,
        args: vec![ta.clone()],
    };
    method(
        st,
        vec,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(st, vec, "length", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        vec,
        "updated",
        vec![Type::Int, Type::Any],
        vec_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        vec,
        ":+",
        vec![Type::Any],
        vec_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "foreach",
        vec![fn1(ta, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let vec_mod = module(
        st,
        st.scala_pkg,
        "Vector",
        "scala/collection/immutable/Vector$",
    );
    let vec_cls = st.module_class_of(vec_mod);
    method(
        st,
        vec_cls,
        "empty",
        vec![],
        Type::Class {
            sym: vec,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let vec_apply = method(
        st,
        vec_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        vec_t.clone(),
        Intrinsic::None,
    );
    let vaa = type_param(st, vec_apply, "A");
    st.get_mut(vec_apply).tparams = vec![vaa];
    st.get_mut(vec_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(vaa)))]],
        ret: Box::new(Type::Class {
            sym: vec,
            args: vec![Type::TypeParam(vaa)],
        }),
    };
    let mems = st.get(vec_cls).members.clone();
    st.get_mut(vec_mod).members.extend(mems);
}

fn add_set(st: &mut SymbolTable) {
    let set = iface(st, st.scala_pkg, "Set", "scala/collection/immutable/Set");
    let sa = type_param(st, set, "A");
    st.get_mut(set).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let set_t = Type::Class {
        sym: set,
        args: vec![ta.clone()],
    };
    method(
        st,
        set,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let set_mod = module(st, st.scala_pkg, "Set", "scala/collection/immutable/Set$");
    let set_cls = st.module_class_of(set_mod);
    method(
        st,
        set_cls,
        "empty",
        vec![],
        Type::Class {
            sym: set,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let set_apply = method(
        st,
        set_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        set_t,
        Intrinsic::None,
    );
    let saa = type_param(st, set_apply, "A");
    st.get_mut(set_apply).tparams = vec![saa];
    st.get_mut(set_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(saa)))]],
        ret: Box::new(Type::Class {
            sym: set,
            args: vec![Type::TypeParam(saa)],
        }),
    };
    let mems = st.get(set_cls).members.clone();
    st.get_mut(set_mod).members.extend(mems);
}

fn add_seq_and_lazylist(st: &mut SymbolTable) {
    let seq = iface(st, st.scala_pkg, "Seq", "scala/collection/immutable/Seq");
    let sa = type_param(st, seq, "A");
    st.get_mut(seq).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let seq_t = Type::Class {
        sym: seq,
        args: vec![ta.clone()],
    };
    method(
        st,
        seq,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        seq,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(st, seq, "length", vec![], Type::Int, Intrinsic::None);
    let seq_mod = module(st, st.scala_pkg, "Seq", "scala/collection/immutable/Seq$");
    let seq_cls = st.module_class_of(seq_mod);
    method(
        st,
        seq_cls,
        "empty",
        vec![],
        Type::Class {
            sym: seq,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let seq_apply = method(
        st,
        seq_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        seq_t.clone(),
        Intrinsic::None,
    );
    let saa = type_param(st, seq_apply, "A");
    st.get_mut(seq_apply).tparams = vec![saa];
    st.get_mut(seq_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(saa)))]],
        ret: Box::new(Type::Class {
            sym: seq,
            args: vec![Type::TypeParam(saa)],
        }),
    };
    let mems = st.get(seq_cls).members.clone();
    st.get_mut(seq_mod).members.extend(mems);

    let ll = class(
        st,
        st.scala_pkg,
        "LazyList",
        "scala/collection/immutable/LazyList",
        &[Type::AnyRef],
    );
    let la = type_param(st, ll, "A");
    st.get_mut(ll).tparams = vec![la];
    let tll = Type::TypeParam(la);
    let ll_t = Type::Class {
        sym: ll,
        args: vec![tll.clone()],
    };
    method(
        st,
        ll,
        "foreach",
        vec![fn1(tll.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, ll, "apply", vec![Type::Int], tll, Intrinsic::None);
    let ll_mod = module(
        st,
        st.scala_pkg,
        "LazyList",
        "scala/collection/immutable/LazyList$",
    );
    let ll_cls = st.module_class_of(ll_mod);
    method(
        st,
        ll_cls,
        "empty",
        vec![],
        Type::Class {
            sym: ll,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ll_apply = method(
        st,
        ll_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        ll_t,
        Intrinsic::None,
    );
    let lla = type_param(st, ll_apply, "A");
    st.get_mut(ll_apply).tparams = vec![lla];
    st.get_mut(ll_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(lla)))]],
        ret: Box::new(Type::Class {
            sym: ll,
            args: vec![Type::TypeParam(lla)],
        }),
    };
    let mems = st.get(ll_cls).members.clone();
    st.get_mut(ll_mod).members.extend(mems);

    // `List` is a `Seq` in 2.13; XML `Elem` takes `Seq[Node]`.
    st.get_mut(st.list_sym).parents.push(Type::Class {
        sym: seq,
        args: vec![],
    });
    // `SeqHasAsJava` takes `scala.collection.Seq`, not `immutable.Seq`.
    let coll_seq = crate::classpath::find_or_stub_java_class(st, "scala/collection/Seq");
    let la = st.get(st.list_sym).tparams[0];
    st.get_mut(st.list_sym).parents.push(Type::Class {
        sym: coll_seq,
        args: vec![Type::TypeParam(la)],
    });
}

/// `scala.collection.View` / `SeqView` against 2.13.16.
///
/// JVM: `SeqOps.view:()SeqView`, `SeqView.map:(Function1)SeqView`,
/// `View`/`SeqView.toList:()List`, `View$.fill(I, Function0)Object`,
/// `View$.iterate(Object, I, Function1)Object`. No private View classfile.
fn add_view(st: &mut SymbolTable) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let view = iface(st, coll, "View", "scala/collection/View");
    let va = type_param(st, view, "A");
    st.get_mut(view).tparams = vec![va];
    let vta = Type::TypeParam(va);
    let list_sym = st.list_sym;
    let view_t = |a: Type| Type::Class {
        sym: view,
        args: vec![a],
    };
    let list_t = |a: Type| Type::Class {
        sym: list_sym,
        args: vec![a],
    };
    method(
        st,
        view,
        "toList",
        vec![],
        list_t(vta.clone()),
        Intrinsic::None,
    );
    let vmap = method(st, view, "map", vec![], Type::Unit, Intrinsic::None);
    let vb = type_param(st, vmap, "B");
    let vf = st.alloc("f", vmap, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(vf).ty = fn1(vta.clone(), Type::TypeParam(vb));
    st.get_mut(vmap).tparams = vec![vb];
    st.get_mut(vmap).params = vec![vf];
    st.get_mut(vmap).paramss = vec![vec![vf]];
    st.get_mut(vmap).ty = Type::Method {
        paramss: vec![vec![fn1(vta.clone(), Type::TypeParam(vb))]],
        ret: Box::new(view_t(Type::TypeParam(vb))),
    };

    let seq_view = iface(st, coll, "SeqView", "scala/collection/SeqView");
    let sa = type_param(st, seq_view, "A");
    st.get_mut(seq_view).tparams = vec![sa];
    let sta = Type::TypeParam(sa);
    st.get_mut(seq_view).parents = vec![
        Type::AnyRef,
        Type::Class {
            sym: view,
            args: vec![sta.clone()],
        },
    ];
    method(
        st,
        seq_view,
        "toList",
        vec![],
        list_t(sta.clone()),
        Intrinsic::None,
    );
    let smap = method(st, seq_view, "map", vec![], Type::Unit, Intrinsic::None);
    let sb = type_param(st, smap, "B");
    let sf = st.alloc("f", smap, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(sf).ty = fn1(sta.clone(), Type::TypeParam(sb));
    st.get_mut(smap).tparams = vec![sb];
    st.get_mut(smap).params = vec![sf];
    st.get_mut(smap).paramss = vec![vec![sf]];
    st.get_mut(smap).ty = Type::Method {
        paramss: vec![vec![fn1(sta.clone(), Type::TypeParam(sb))]],
        ret: Box::new(Type::Class {
            sym: seq_view,
            args: vec![Type::TypeParam(sb)],
        }),
    };

    if let Some(la) = st.get(st.list_sym).tparams.first().copied() {
        method(
            st,
            st.list_sym,
            "view",
            vec![],
            Type::Class {
                sym: seq_view,
                args: vec![Type::TypeParam(la)],
            },
            Intrinsic::None,
        );
    }

    let view_mod = module(st, coll, "View", "scala/collection/View$");
    let view_cls = st.module_class_of(view_mod);

    let fill = method(st, view_cls, "fill", vec![], Type::Unit, Intrinsic::None);
    let fa = type_param(st, fill, "A");
    let n = st.alloc("n", fill, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(n).ty = Type::Int;
    let elem = st.alloc("elem", fill, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(elem).ty = Type::ByName(Box::new(Type::TypeParam(fa)));
    st.get_mut(fill).tparams = vec![fa];
    st.get_mut(fill).params = vec![n, elem];
    st.get_mut(fill).paramss = vec![vec![n], vec![elem]];
    st.get_mut(fill).ty = Type::Method {
        paramss: vec![
            vec![Type::Int],
            vec![Type::ByName(Box::new(Type::TypeParam(fa)))],
        ],
        ret: Box::new(view_t(Type::TypeParam(fa))),
    };
    st.get_mut(fill).jvm_name = "(ILscala/Function0;)Ljava/lang/Object;".into();

    let iterate = method(st, view_cls, "iterate", vec![], Type::Unit, Intrinsic::None);
    let ia = type_param(st, iterate, "A");
    let start = st.alloc(
        "start",
        iterate,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(start).ty = Type::TypeParam(ia);
    let len = st.alloc(
        "len",
        iterate,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(len).ty = Type::Int;
    let f = st.alloc("f", iterate, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(Type::TypeParam(ia), Type::TypeParam(ia));
    st.get_mut(iterate).tparams = vec![ia];
    st.get_mut(iterate).params = vec![start, len, f];
    st.get_mut(iterate).paramss = vec![vec![start, len], vec![f]];
    st.get_mut(iterate).ty = Type::Method {
        paramss: vec![
            vec![Type::TypeParam(ia), Type::Int],
            vec![fn1(Type::TypeParam(ia), Type::TypeParam(ia))],
        ],
        ret: Box::new(view_t(Type::TypeParam(ia))),
    };
    st.get_mut(iterate).jvm_name =
        "(Ljava/lang/Object;ILscala/Function1;)Ljava/lang/Object;".into();

    let mems = st.get(view_cls).members.clone();
    st.get_mut(view_mod).members.extend(mems);
}

fn add_indexedseq_and_queue(st: &mut SymbolTable) {
    let idx = iface(
        st,
        st.scala_pkg,
        "IndexedSeq",
        "scala/collection/immutable/IndexedSeq",
    );
    let ia = type_param(st, idx, "A");
    st.get_mut(idx).tparams = vec![ia];
    let ta = Type::TypeParam(ia);
    let idx_t = Type::Class {
        sym: idx,
        args: vec![ta.clone()],
    };
    method(
        st,
        idx,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    let idx_mod = module(
        st,
        st.scala_pkg,
        "IndexedSeq",
        "scala/collection/immutable/IndexedSeq$",
    );
    let idx_cls = st.module_class_of(idx_mod);
    method(
        st,
        idx_cls,
        "empty",
        vec![],
        Type::Class {
            sym: idx,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let idx_apply = method(
        st,
        idx_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        idx_t.clone(),
        Intrinsic::None,
    );
    let iaa = type_param(st, idx_apply, "A");
    st.get_mut(idx_apply).tparams = vec![iaa];
    st.get_mut(idx_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(iaa)))]],
        ret: Box::new(Type::Class {
            sym: idx,
            args: vec![Type::TypeParam(iaa)],
        }),
    };
    let mems = st.get(idx_cls).members.clone();
    st.get_mut(idx_mod).members.extend(mems);

    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let imm = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let queue = class(
        st,
        imm,
        "Queue",
        "scala/collection/immutable/Queue",
        &[Type::AnyRef],
    );
    let qa = type_param(st, queue, "A");
    st.get_mut(queue).tparams = vec![qa];
    let tq = Type::TypeParam(qa);
    let queue_t = Type::Class {
        sym: queue,
        args: vec![tq.clone()],
    };
    method(
        st,
        queue,
        "enqueue",
        vec![Type::Any],
        queue_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        queue,
        "dequeue",
        vec![],
        Type::Class {
            sym: tuple2,
            args: vec![tq.clone(), queue_t.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        queue,
        "apply",
        vec![Type::Int],
        tq.clone(),
        Intrinsic::None,
    );
    let queue_mod = module(st, imm, "Queue", "scala/collection/immutable/Queue$");
    let queue_cls = st.module_class_of(queue_mod);
    method(
        st,
        queue_cls,
        "empty",
        vec![],
        Type::Class {
            sym: queue,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let q_apply = method(
        st,
        queue_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        queue_t.clone(),
        Intrinsic::None,
    );
    let qaa = type_param(st, q_apply, "A");
    st.get_mut(q_apply).tparams = vec![qaa];
    st.get_mut(q_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(qaa)))]],
        ret: Box::new(Type::Class {
            sym: queue,
            args: vec![Type::TypeParam(qaa)],
        }),
    };
    let mems = st.get(queue_cls).members.clone();
    st.get_mut(queue_mod).members.extend(mems);
}

fn add_array_buffer(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = class(
        st,
        mutp,
        "ArrayBuffer",
        "scala/collection/mutable/ArrayBuffer",
        &[Type::AnyRef],
    );
    let ba = type_param(st, buf, "A");
    st.get_mut(buf).tparams = vec![ba];
    let ta = Type::TypeParam(ba);
    let buf_t = Type::Class {
        sym: buf,
        args: vec![ta.clone()],
    };
    method(
        st,
        buf,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "update",
        vec![Type::Int, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "+=",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );
    let buf_mod = module(
        st,
        mutp,
        "ArrayBuffer",
        "scala/collection/mutable/ArrayBuffer$",
    );
    let buf_cls = st.module_class_of(buf_mod);
    method(
        st,
        buf_cls,
        "empty",
        vec![],
        Type::Class {
            sym: buf,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let buf_apply = method(
        st,
        buf_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        buf_t.clone(),
        Intrinsic::None,
    );
    let baa = type_param(st, buf_apply, "A");
    st.get_mut(buf_apply).tparams = vec![baa];
    st.get_mut(buf_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(baa)))]],
        ret: Box::new(Type::Class {
            sym: buf,
            args: vec![Type::TypeParam(baa)],
        }),
    };
    let mems = st.get(buf_cls).members.clone();
    st.get_mut(buf_mod).members.extend(mems);
}

fn add_list_buffer(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = class(
        st,
        mutp,
        "ListBuffer",
        "scala/collection/mutable/ListBuffer",
        &[Type::AnyRef],
    );
    let ba = type_param(st, buf, "A");
    st.get_mut(buf).tparams = vec![ba];
    let ta = Type::TypeParam(ba);
    let buf_t = Type::Class {
        sym: buf,
        args: vec![ta.clone()],
    };
    method(
        st,
        buf,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "+=",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );
    let buf_mod = module(
        st,
        mutp,
        "ListBuffer",
        "scala/collection/mutable/ListBuffer$",
    );
    let buf_cls = st.module_class_of(buf_mod);
    method(
        st,
        buf_cls,
        "empty",
        vec![],
        Type::Class {
            sym: buf,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let buf_apply = method(
        st,
        buf_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        buf_t.clone(),
        Intrinsic::None,
    );
    let baa = type_param(st, buf_apply, "A");
    st.get_mut(buf_apply).tparams = vec![baa];
    st.get_mut(buf_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(baa)))]],
        ret: Box::new(Type::Class {
            sym: buf,
            args: vec![Type::TypeParam(baa)],
        }),
    };
    let mems = st.get(buf_cls).members.clone();
    st.get_mut(buf_mod).members.extend(mems);
}

fn add_array_deque(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let deq = class(
        st,
        mutp,
        "ArrayDeque",
        "scala/collection/mutable/ArrayDeque",
        &[Type::AnyRef],
    );
    let da = type_param(st, deq, "A");
    st.get_mut(deq).tparams = vec![da];
    let ta = Type::TypeParam(da);
    let deq_t = Type::Class {
        sym: deq,
        args: vec![ta.clone()],
    };
    method(
        st,
        deq,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        deq,
        "+=",
        vec![Type::Any],
        deq_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        deq,
        "prepend",
        vec![Type::Any],
        deq_t.clone(),
        Intrinsic::None,
    );
    let deq_mod = module(
        st,
        mutp,
        "ArrayDeque",
        "scala/collection/mutable/ArrayDeque$",
    );
    let deq_cls = st.module_class_of(deq_mod);
    let deq_empty = method(
        st,
        deq_cls,
        "empty",
        vec![],
        Type::Class {
            sym: deq,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ea = type_param(st, deq_empty, "A");
    st.get_mut(deq_empty).tparams = vec![ea];
    st.get_mut(deq_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: deq,
            args: vec![Type::TypeParam(ea)],
        }),
    };
    let deq_apply = method(
        st,
        deq_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        deq_t.clone(),
        Intrinsic::None,
    );
    let daa = type_param(st, deq_apply, "A");
    st.get_mut(deq_apply).tparams = vec![daa];
    st.get_mut(deq_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(daa)))]],
        ret: Box::new(Type::Class {
            sym: deq,
            args: vec![Type::TypeParam(daa)],
        }),
    };
    let mems = st.get(deq_cls).members.clone();
    st.get_mut(deq_mod).members.extend(mems);
}

fn add_hash_map(st: &mut SymbolTable) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let hm = class(
        st,
        mutp,
        "HashMap",
        "scala/collection/mutable/HashMap",
        &[Type::AnyRef],
    );
    let mk = type_param(st, hm, "K");
    let mv = type_param(st, hm, "V");
    st.get_mut(hm).tparams = vec![mk, mv];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let hm_t = Type::Class {
        sym: hm,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk, tv.clone()],
    };
    method(
        st,
        hm,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        hm,
        "get",
        vec![Type::Any],
        Type::Class {
            sym: st.option_sym,
            args: vec![tv.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        hm,
        "update",
        vec![Type::Any, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, hm, "+=", vec![Type::Any], hm_t.clone(), Intrinsic::None);
    let hm_mod = module(st, mutp, "HashMap", "scala/collection/mutable/HashMap$");
    let hm_cls = st.module_class_of(hm_mod);
    let hm_empty = method(
        st,
        hm_cls,
        "empty",
        vec![],
        Type::Class {
            sym: hm,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let ek = type_param(st, hm_empty, "K");
    let ev = type_param(st, hm_empty, "V");
    st.get_mut(hm_empty).tparams = vec![ek, ev];
    st.get_mut(hm_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: hm,
            args: vec![Type::TypeParam(ek), Type::TypeParam(ev)],
        }),
    };
    let hm_apply = method(
        st,
        hm_cls,
        "apply",
        vec![Type::Repeated(Box::new(pair.clone()))],
        hm_t.clone(),
        Intrinsic::None,
    );
    let hak = type_param(st, hm_apply, "K");
    let hav = type_param(st, hm_apply, "V");
    st.get_mut(hm_apply).tparams = vec![hak, hav];
    let hm_pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(hak), Type::TypeParam(hav)],
    };
    st.get_mut(hm_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(hm_pair))]],
        ret: Box::new(Type::Class {
            sym: hm,
            args: vec![Type::TypeParam(hak), Type::TypeParam(hav)],
        }),
    };
    let mems = st.get(hm_cls).members.clone();
    st.get_mut(hm_mod).members.extend(mems);
}

fn add_hash_set(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let hs = class(
        st,
        mutp,
        "HashSet",
        "scala/collection/mutable/HashSet",
        &[Type::AnyRef],
    );
    let sa = type_param(st, hs, "A");
    st.get_mut(hs).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let hs_t = Type::Class {
        sym: hs,
        args: vec![ta.clone()],
    };
    method(
        st,
        hs,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, hs, "+=", vec![Type::Any], hs_t.clone(), Intrinsic::None);
    let hs_mod = module(st, mutp, "HashSet", "scala/collection/mutable/HashSet$");
    let hs_cls = st.module_class_of(hs_mod);
    let hs_empty = method(
        st,
        hs_cls,
        "empty",
        vec![],
        Type::Class {
            sym: hs,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ea = type_param(st, hs_empty, "A");
    st.get_mut(hs_empty).tparams = vec![ea];
    st.get_mut(hs_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: hs,
            args: vec![Type::TypeParam(ea)],
        }),
    };
    let hs_apply = method(
        st,
        hs_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        hs_t.clone(),
        Intrinsic::None,
    );
    let haa = type_param(st, hs_apply, "A");
    st.get_mut(hs_apply).tparams = vec![haa];
    st.get_mut(hs_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(haa)))]],
        ret: Box::new(Type::Class {
            sym: hs,
            args: vec![Type::TypeParam(haa)],
        }),
    };
    let mems = st.get(hs_cls).members.clone();
    st.get_mut(hs_mod).members.extend(mems);
}

fn add_linked_hash_map(st: &mut SymbolTable) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let lhm = class(
        st,
        mutp,
        "LinkedHashMap",
        "scala/collection/mutable/LinkedHashMap",
        &[Type::AnyRef],
    );
    let mk = type_param(st, lhm, "K");
    let mv = type_param(st, lhm, "V");
    st.get_mut(lhm).tparams = vec![mk, mv];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let lhm_t = Type::Class {
        sym: lhm,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk, tv.clone()],
    };
    method(
        st,
        lhm,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lhm,
        "update",
        vec![Type::Any, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        lhm,
        "+=",
        vec![Type::Any],
        lhm_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lhm,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let lhm_mod = module(
        st,
        mutp,
        "LinkedHashMap",
        "scala/collection/mutable/LinkedHashMap$",
    );
    let lhm_cls = st.module_class_of(lhm_mod);
    let lhm_empty = method(
        st,
        lhm_cls,
        "empty",
        vec![],
        Type::Class {
            sym: lhm,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let ek = type_param(st, lhm_empty, "K");
    let ev = type_param(st, lhm_empty, "V");
    st.get_mut(lhm_empty).tparams = vec![ek, ev];
    st.get_mut(lhm_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: lhm,
            args: vec![Type::TypeParam(ek), Type::TypeParam(ev)],
        }),
    };
    let lhm_apply = method(
        st,
        lhm_cls,
        "apply",
        vec![Type::Repeated(Box::new(pair.clone()))],
        lhm_t.clone(),
        Intrinsic::None,
    );
    let lak = type_param(st, lhm_apply, "K");
    let lav = type_param(st, lhm_apply, "V");
    st.get_mut(lhm_apply).tparams = vec![lak, lav];
    let lhm_pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(lak), Type::TypeParam(lav)],
    };
    st.get_mut(lhm_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(lhm_pair))]],
        ret: Box::new(Type::Class {
            sym: lhm,
            args: vec![Type::TypeParam(lak), Type::TypeParam(lav)],
        }),
    };
    let mems = st.get(lhm_cls).members.clone();
    st.get_mut(lhm_mod).members.extend(mems);
}

fn add_linked_hash_set(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let lhs = class(
        st,
        mutp,
        "LinkedHashSet",
        "scala/collection/mutable/LinkedHashSet",
        &[Type::AnyRef],
    );
    let sa = type_param(st, lhs, "A");
    st.get_mut(lhs).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let lhs_t = Type::Class {
        sym: lhs,
        args: vec![ta.clone()],
    };
    method(
        st,
        lhs,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        lhs,
        "+=",
        vec![Type::Any],
        lhs_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lhs,
        "foreach",
        vec![fn1(ta, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let lhs_mod = module(
        st,
        mutp,
        "LinkedHashSet",
        "scala/collection/mutable/LinkedHashSet$",
    );
    let lhs_cls = st.module_class_of(lhs_mod);
    let lhs_empty = method(
        st,
        lhs_cls,
        "empty",
        vec![],
        Type::Class {
            sym: lhs,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ea = type_param(st, lhs_empty, "A");
    st.get_mut(lhs_empty).tparams = vec![ea];
    st.get_mut(lhs_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: lhs,
            args: vec![Type::TypeParam(ea)],
        }),
    };
    let lhs_apply = method(
        st,
        lhs_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        lhs_t.clone(),
        Intrinsic::None,
    );
    let haa = type_param(st, lhs_apply, "A");
    st.get_mut(lhs_apply).tparams = vec![haa];
    st.get_mut(lhs_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(haa)))]],
        ret: Box::new(Type::Class {
            sym: lhs,
            args: vec![Type::TypeParam(haa)],
        }),
    };
    let mems = st.get(lhs_cls).members.clone();
    st.get_mut(lhs_mod).members.extend(mems);
}

/// Base `scala.collection.mutable.StringBuilder` class symbol. The full
/// member set (constructors, `append` overloads, `+=`, `insert`, `reverse`,
/// ...) is added by `prelude_text::add_string_builder_full`, which reuses
/// this same symbol (and aliases it under `scala` for the bare name) rather
/// than declaring a second, conflicting one.
fn add_string_builder(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    class(
        st,
        mutp,
        "StringBuilder",
        "scala/collection/mutable/StringBuilder",
        &[Type::AnyRef],
    );
}

fn add_either(st: &mut SymbolTable) {
    let either = class(
        st,
        st.scala_pkg,
        "Either",
        "scala/util/Either",
        &[Type::AnyRef],
    );
    let ea = type_param(st, either, "A");
    let eb = type_param(st, either, "B");
    st.get_mut(either).tparams = vec![ea, eb];
    let tb = Type::TypeParam(eb);
    let either_t = Type::Class {
        sym: either,
        args: vec![Type::TypeParam(ea), tb.clone()],
    };
    method(st, either, "isLeft", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        either,
        "getOrElse",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "map",
        vec![fn1(tb, Type::Any)],
        either_t.clone(),
        Intrinsic::None,
    );

    // nsc: `class Left[+A, +B](value: A) extends Either[A, B]`
    let left = class(
        st,
        st.scala_pkg,
        "Left",
        "scala/util/Left",
        &[either_t.clone()],
    );
    let la = type_param(st, left, "A");
    let lb = type_param(st, left, "B");
    st.get_mut(left).tparams = vec![la, lb];
    st.get_mut(left).parents = vec![Type::Class {
        sym: either,
        args: vec![Type::TypeParam(la), Type::TypeParam(lb)],
    }];
    let lf = st.alloc("value", left, SymKind::Term, Flags::FINAL, "");
    st.get_mut(lf).ty = Type::TypeParam(la);
    st.get_mut(left).ctor_fields = vec![lf];
    // The field is private in the library, so `case Left(s)` has to read it
    // through the accessor -- without this the pattern emitted a `getfield`
    // and threw `IllegalAccessError`. Same reason as `Success.value`.
    method(
        st,
        left,
        "value",
        vec![],
        Type::TypeParam(la),
        Intrinsic::None,
    );
    let left_mod = module(st, st.scala_pkg, "Left", "scala/util/Left$");
    let left_cls = st.module_class_of(left_mod);
    let left_apply = method(
        st,
        left_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: left,
            args: vec![Type::TypeParam(la), Type::TypeParam(lb)],
        },
        Intrinsic::None,
    );
    st.get_mut(left_apply).tparams = vec![la, lb];
    let mems = st.get(left_cls).members.clone();
    st.get_mut(left_mod).members.extend(mems);

    // nsc: `class Right[+A, +B](value: B) extends Either[A, B]`
    let right = class(st, st.scala_pkg, "Right", "scala/util/Right", &[either_t]);
    let ra = type_param(st, right, "A");
    let rb = type_param(st, right, "B");
    st.get_mut(right).tparams = vec![ra, rb];
    st.get_mut(right).parents = vec![Type::Class {
        sym: either,
        args: vec![Type::TypeParam(ra), Type::TypeParam(rb)],
    }];
    let rf = st.alloc("value", right, SymKind::Term, Flags::FINAL, "");
    st.get_mut(rf).ty = Type::TypeParam(rb);
    st.get_mut(right).ctor_fields = vec![rf];
    method(
        st,
        right,
        "value",
        vec![],
        Type::TypeParam(rb),
        Intrinsic::None,
    );
    let right_mod = module(st, st.scala_pkg, "Right", "scala/util/Right$");
    let right_cls = st.module_class_of(right_mod);
    let right_apply = method(
        st,
        right_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: right,
            args: vec![Type::TypeParam(ra), Type::TypeParam(rb)],
        },
        Intrinsic::None,
    );
    st.get_mut(right_apply).tparams = vec![ra, rb];
    let mems = st.get(right_cls).members.clone();
    st.get_mut(right_mod).members.extend(mems);
}

fn add_try(st: &mut SymbolTable, throwable: SymbolId) {
    let try_c = class(st, st.scala_pkg, "Try", "scala/util/Try", &[Type::AnyRef]);
    let tt = type_param(st, try_c, "T");
    st.get_mut(try_c).tparams = vec![tt];
    let t_ty = Type::TypeParam(tt);
    let try_t = Type::Class {
        sym: try_c,
        args: vec![t_ty.clone()],
    };
    method(
        st,
        try_c,
        "getOrElse",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "map",
        vec![fn1(t_ty, Type::Any)],
        try_t.clone(),
        Intrinsic::None,
    );

    let try_mod = module(st, st.scala_pkg, "Try", "scala/util/Try$");
    let try_cls = st.module_class_of(try_mod);
    method(
        st,
        try_cls,
        "apply",
        vec![Type::ByName(Box::new(Type::Any))],
        try_t.clone(),
        Intrinsic::None,
    );
    let mems = st.get(try_cls).members.clone();
    st.get_mut(try_mod).members.extend(mems);

    let success = class(
        st,
        st.scala_pkg,
        "Success",
        "scala/util/Success",
        &[try_t.clone()],
    );
    let sa = type_param(st, success, "T");
    st.get_mut(success).tparams = vec![sa];
    let sf = st.alloc("value", success, SymKind::Term, Flags::FINAL, "");
    st.get_mut(sf).ty = Type::TypeParam(sa);
    st.get_mut(success).ctor_fields = vec![sf];
    // The field is private in the library; a pattern reads it through this.
    method(
        st,
        success,
        "value",
        vec![],
        Type::TypeParam(sa),
        Intrinsic::None,
    );
    let success_mod = module(st, st.scala_pkg, "Success", "scala/util/Success$");
    let success_cls = st.module_class_of(success_mod);
    // `def apply[T](value: T): Success[T]`. A raw `Success` conformed to
    // nothing: `def a[R](…): Try[R] = Success(f)` reported
    // `found: Success required: Try[R]`.
    let sm = method(
        st,
        success_cls,
        "apply",
        vec![Type::Any],
        Type::Any,
        Intrinsic::None,
    );
    let smt = type_param(st, sm, "T");
    st.get_mut(sm).tparams = vec![smt];
    st.get_mut(sm).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(smt)]],
        ret: Box::new(Type::Class {
            sym: success,
            args: vec![Type::TypeParam(smt)],
        }),
    };
    let mems = st.get(success_cls).members.clone();
    st.get_mut(success_mod).members.extend(mems);

    let throwable_ty = Type::Class {
        sym: throwable,
        args: vec![],
    };
    let throwable_ty2 = throwable_ty.clone();
    let failure = class(st, st.scala_pkg, "Failure", "scala/util/Failure", &[try_t]);
    let fa = type_param(st, failure, "T");
    st.get_mut(failure).tparams = vec![fa];
    let ff = st.alloc("exception", failure, SymKind::Term, Flags::FINAL, "");
    st.get_mut(ff).ty = throwable_ty.clone();
    st.get_mut(failure).ctor_fields = vec![ff];
    // The field is private in the library; a pattern reads it through this.
    method(
        st,
        failure,
        "exception",
        vec![],
        throwable_ty.clone(),
        Intrinsic::None,
    );
    let failure_mod = module(st, st.scala_pkg, "Failure", "scala/util/Failure$");
    let failure_cls = st.module_class_of(failure_mod);
    // `def apply[T](exception: Throwable): Failure[T]`. `T` appears in no
    // parameter, so only the expected type (or `Nothing`, which `Try`'s
    // covariance makes harmless) can pin it -- but a *raw* `Failure` could not
    // be pinned at all.
    let fm = method(
        st,
        failure_cls,
        "apply",
        vec![throwable_ty],
        Type::Any,
        Intrinsic::None,
    );
    let fmt = type_param(st, fm, "T");
    st.get_mut(fm).tparams = vec![fmt];
    st.get_mut(fm).ty = Type::Method {
        paramss: vec![vec![throwable_ty2]],
        ret: Box::new(Type::Class {
            sym: failure,
            args: vec![Type::TypeParam(fmt)],
        }),
    };
    let mems = st.get(failure_cls).members.clone();
    st.get_mut(failure_mod).members.extend(mems);
}

/// `scala.util.control.Breaks` / `Breaks$` against scala-library 2.13.16.
/// nsc accepts `import Breaks._`, `Breaks.breakable { ... }`, and `new Breaks`.
fn add_breaks(st: &mut SymbolTable) {
    let control = crate::classpath::ensure_package(st, "scala/util/control");
    let breaks = st.alloc(
        "Breaks",
        control,
        crate::symbol::SymKind::Class,
        Flags::EMPTY,
        "scala/util/control/Breaks",
    );
    st.get_mut(breaks).parents = vec![Type::AnyRef];
    st.get_mut(breaks).ty = Type::Class {
        sym: breaks,
        args: vec![],
    };
    let try_block = iface(st, breaks, "TryBlock", "scala/util/control/Breaks$TryBlock");
    let tt = type_param(st, try_block, "T");
    st.get_mut(try_block).tparams = vec![tt];
    let cb = method(
        st,
        try_block,
        "catchBreak",
        vec![Type::ByName(Box::new(Type::TypeParam(tt)))],
        Type::TypeParam(tt),
        Intrinsic::None,
    );
    st.get_mut(cb).jvm_name = "(Lscala/Function0;)Ljava/lang/Object;".into();
    add_breaks_members(st, breaks, try_block);
    method(
        st,
        breaks,
        "<init>",
        vec![],
        Type::Class {
            sym: breaks,
            args: vec![],
        },
        Intrinsic::None,
    );
    let breaks_mod = module_extending(
        st,
        control,
        "Breaks",
        "scala/util/control/Breaks$",
        Type::Class {
            sym: breaks,
            args: vec![],
        },
    );
    let mcls = st.module_class_of(breaks_mod);
    add_breaks_members(st, mcls, try_block);
    let mems = st.get(mcls).members.clone();
    st.get_mut(breaks_mod).members.extend(mems);
}

fn add_breaks_members(st: &mut SymbolTable, owner: SymbolId, try_block: SymbolId) {
    method(
        st,
        owner,
        "breakable",
        vec![Type::ByName(Box::new(Type::Unit))],
        Type::Unit,
        Intrinsic::None,
    );
    let br = method(st, owner, "break", vec![], Type::Nothing, Intrinsic::None);
    st.get_mut(br).jvm_name = "()Lscala/runtime/Nothing$;".into();
    // nsc 2.13.16: `def tryBreakable[T](op: => T): Breaks.TryBlock[T]`
    let tb = method(
        st,
        owner,
        "tryBreakable",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let t = type_param(st, tb, "T");
    let op = st.alloc("op", tb, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = Type::ByName(Box::new(Type::TypeParam(t)));
    st.get_mut(tb).tparams = vec![t];
    st.get_mut(tb).params = vec![op];
    st.get_mut(tb).paramss = vec![vec![op]];
    st.get_mut(tb).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::TypeParam(t)))]],
        ret: Box::new(Type::Class {
            sym: try_block,
            args: vec![Type::TypeParam(t)],
        }),
    };
    st.get_mut(tb).jvm_name = "(Lscala/Function0;)Lscala/util/control/Breaks$TryBlock;".into();
}

/// `scala.math.BigInt` + companion `BigInt$` against scala-library 2.13.16.
///
/// JVM: `BigInt$.apply(I)` / `apply(Ljava/lang/String;)` /
/// `int2bigInt(I)`（IMPLICIT） / instance `$plus` / `$times`.
fn add_big_int(st: &mut SymbolTable) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let cls = class(st, math, "BigInt", "scala/math/BigInt", &[Type::AnyRef]);
    let this_t = Type::Class {
        sym: cls,
        args: vec![],
    };
    method(
        st,
        cls,
        "+",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        cls,
        "*",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    let big_mod = module(st, math, "BigInt", "scala/math/BigInt$");
    let mcls = st.module_class_of(big_mod);
    method(
        st,
        mcls,
        "apply",
        vec![Type::Int],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        mcls,
        "apply",
        vec![Type::String],
        this_t.clone(),
        Intrinsic::None,
    );
    let conv = method(
        st,
        mcls,
        "int2bigInt",
        vec![Type::Int],
        this_t,
        Intrinsic::None,
    );
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    let mems = st.get(mcls).members.clone();
    st.get_mut(big_mod).members.extend(mems);
}

/// `scala.math.BigDecimal` + companion。small extra。
fn add_big_decimal(st: &mut SymbolTable) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let cls = class(
        st,
        math,
        "BigDecimal",
        "scala/math/BigDecimal",
        &[Type::AnyRef],
    );
    let this_t = Type::Class {
        sym: cls,
        args: vec![],
    };
    method(
        st,
        cls,
        "+",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        cls,
        "*",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    let big_mod = module(st, math, "BigDecimal", "scala/math/BigDecimal$");
    let mcls = st.module_class_of(big_mod);
    method(
        st,
        mcls,
        "apply",
        vec![Type::Int],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        mcls,
        "apply",
        vec![Type::String],
        this_t.clone(),
        Intrinsic::None,
    );
    let mems = st.get(mcls).members.clone();
    st.get_mut(big_mod).members.extend(mems);
}

/// `scala.util.chaining` (`package$chaining$`) + `ChainingOps` against 2.13.16.
///
/// `import scala.util.chaining._` brings IMPLICIT `scalaUtilChainingOps`.
/// JVM: `pipe$extension` / `tap$extension(Object, Function1)Object`.
fn add_chaining(st: &mut SymbolTable) {
    let util = crate::classpath::ensure_package(st, "scala/util");
    let ops = class(
        st,
        util,
        "ChainingOps",
        "scala/util/ChainingOps",
        &[Type::AnyVal],
    );
    let a = type_param(st, ops, "A");
    st.get_mut(ops).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let self_f = st.alloc("self", ops, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(self_f).ty = ta.clone();
    st.get_mut(ops).ctor_fields = vec![self_f];

    let pipe = method(st, ops, "pipe", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, pipe, "B");
    let f = st.alloc("f", pipe, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(b));
    st.get_mut(pipe).tparams = vec![b];
    st.get_mut(pipe).params = vec![f];
    st.get_mut(pipe).paramss = vec![vec![f]];
    st.get_mut(pipe).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(b))]],
        ret: Box::new(Type::TypeParam(b)),
    };

    let tap = method(st, ops, "tap", vec![], Type::Unit, Intrinsic::None);
    let u = type_param(st, tap, "U");
    let g = st.alloc("f", tap, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(g).ty = fn1(ta.clone(), Type::TypeParam(u));
    st.get_mut(tap).tparams = vec![u];
    st.get_mut(tap).params = vec![g];
    st.get_mut(tap).paramss = vec![vec![g]];
    st.get_mut(tap).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(u))]],
        ret: Box::new(ta.clone()),
    };

    let chaining = module(st, util, "chaining", "scala/util/package$chaining$");
    let mcls = st.module_class_of(chaining);
    let conv = method(
        st,
        mcls,
        "scalaUtilChainingOps",
        vec![Type::Any],
        Type::Class {
            sym: ops,
            args: vec![],
        },
        Intrinsic::Identity,
    );
    let ca = type_param(st, conv, "A");
    let cta = Type::TypeParam(ca);
    st.get_mut(conv).tparams = vec![ca];
    st.get_mut(conv).ty = Type::Method {
        paramss: vec![vec![cta.clone()]],
        ret: Box::new(Type::Class {
            sym: ops,
            args: vec![cta],
        }),
    };
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    let mems = st.get(mcls).members.clone();
    st.get_mut(chaining).members.extend(mems);
}

/// `scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources`
/// + `Releasable[-R]` against scala-library 2.13.16.
///
/// nsc 2.13.16 JVM:
/// `Using$.resource(Object, Function1, Using$Releasable)Object`,
/// `Using$.apply(Function0, Function1, Using$Releasable)Try`,
/// `Using$Manager$.apply(Function1)Try`,
/// `Using$Manager.apply/acquire(Object, Using$Releasable)`,
/// `Using$.resources` 2–4 resource overloads (Function2/3/4).
/// Implicit `Using$Releasable$AutoCloseableIsReleasable$.MODULE$`.
fn add_using(st: &mut SymbolTable) {
    let util = crate::classpath::ensure_package(st, "scala/util");
    let java_lang = crate::classpath::ensure_package(st, "java/lang");
    let auto_closeable = st
        .lookup_member(java_lang, "AutoCloseable")
        .into_iter()
        .find(|&id| st.get(id).is_class_like())
        .unwrap_or_else(|| {
            let ac = iface(st, java_lang, "AutoCloseable", "java/lang/AutoCloseable");
            mark_java(st, ac);
            let close = st.alloc(
                "close",
                ac,
                crate::symbol::SymKind::Method,
                Flags::ABSTRACT,
                "",
            );
            st.get_mut(close).ty = Type::Method {
                paramss: Vec::new(),
                ret: Box::new(Type::Unit),
            };
            ac
        });

    let releasable = iface(st, util, "Releasable", "scala/util/Using$Releasable");
    let r = type_param(st, releasable, "R");
    st.get_mut(r).flags = st.get(r).flags.with(Flags::CONTRAVARIANT);
    st.get_mut(releasable).tparams = vec![r];
    let release = st.alloc(
        "release",
        releasable,
        crate::symbol::SymKind::Method,
        Flags::ABSTRACT,
        "",
    );
    st.get_mut(release).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(r)]],
        ret: Box::new(Type::Unit),
    };

    let rel_mod = module(st, util, "Releasable", "scala/util/Using$Releasable$");
    let rel_cls = st.module_class_of(rel_mod);
    add_ordering_instance(
        st,
        rel_cls,
        releasable,
        "AutoCloseableIsReleasable",
        "scala/util/Using$Releasable$AutoCloseableIsReleasable$",
        Type::Class {
            sym: auto_closeable,
            args: vec![],
        },
    );
    let mems = st.get(rel_cls).members.clone();
    st.get_mut(rel_mod).members.extend(mems);

    let using_mod = module(st, util, "Using", "scala/util/Using$");
    let using_cls = st.module_class_of(using_mod);
    let res = method(
        st,
        using_cls,
        "resource",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let rr = type_param(st, res, "R");
    let aa = type_param(st, res, "A");
    st.get_mut(res).tparams = vec![rr, aa];
    let r_t = Type::TypeParam(rr);
    let a_t = Type::TypeParam(aa);
    let resource = st.alloc(
        "resource",
        res,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(resource).ty = r_t.clone();
    let f = st.alloc("f", res, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(r_t.clone(), a_t.clone());
    let ev = st.alloc(
        "releasable",
        res,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: releasable,
        args: vec![r_t.clone()],
    };
    st.get_mut(res).params = vec![resource, f, ev];
    st.get_mut(res).paramss = vec![vec![resource], vec![f], vec![ev]];
    st.get_mut(res).ty = Type::Method {
        paramss: vec![
            vec![r_t.clone()],
            vec![fn1(r_t.clone(), a_t.clone())],
            vec![Type::Class {
                sym: releasable,
                args: vec![r_t],
            }],
        ],
        ret: Box::new(a_t),
    };

    let try_c = st
        .lookup_member(st.scala_pkg, "Try")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .expect("Try");

    // nsc `Using.apply[R, A](resource: => R)(f: R => A)(implicit Releasable[R]): Try[A]`
    // JVM: `Using$.apply(Function0, Function1, Using$Releasable)Try`
    let app = method(st, using_cls, "apply", vec![], Type::Unit, Intrinsic::None);
    let ar = type_param(st, app, "R");
    let aa2 = type_param(st, app, "A");
    st.get_mut(app).tparams = vec![ar, aa2];
    let ar_t = Type::TypeParam(ar);
    let aa2_t = Type::TypeParam(aa2);
    let app_res = st.alloc(
        "resource",
        app,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(app_res).ty = Type::ByName(Box::new(ar_t.clone()));
    let app_f = st.alloc("f", app, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(app_f).ty = fn1(ar_t.clone(), aa2_t.clone());
    let app_ev = st.alloc(
        "releasable",
        app,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(app_ev).ty = Type::Class {
        sym: releasable,
        args: vec![ar_t.clone()],
    };
    st.get_mut(app).params = vec![app_res, app_f, app_ev];
    st.get_mut(app).paramss = vec![vec![app_res], vec![app_f], vec![app_ev]];
    st.get_mut(app).ty = Type::Method {
        paramss: vec![
            vec![Type::ByName(Box::new(ar_t.clone()))],
            vec![fn1(ar_t.clone(), aa2_t.clone())],
            vec![Type::Class {
                sym: releasable,
                args: vec![ar_t],
            }],
        ],
        ret: Box::new(Type::Class {
            sym: try_c,
            args: vec![aa2_t],
        }),
    };
    st.get_mut(app).jvm_name =
        "(Lscala/Function0;Lscala/Function1;Lscala/util/Using$Releasable;)Lscala/util/Try;".into();

    let manager_cls = class(
        st,
        using_cls,
        "Manager",
        "scala/util/Using$Manager",
        &[Type::AnyRef],
    );
    method(
        st,
        manager_cls,
        "<init>",
        vec![],
        Type::Class {
            sym: manager_cls,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mgr_t = Type::Class {
        sym: manager_cls,
        args: vec![],
    };

    let mgr_app = method(
        st,
        manager_cls,
        "apply",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let mr = type_param(st, mgr_app, "R");
    st.get_mut(mgr_app).tparams = vec![mr];
    let mr_t = Type::TypeParam(mr);
    let mgr_res = st.alloc(
        "resource",
        mgr_app,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(mgr_res).ty = mr_t.clone();
    let mgr_ev = st.alloc(
        "releasable",
        mgr_app,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(mgr_ev).ty = Type::Class {
        sym: releasable,
        args: vec![mr_t.clone()],
    };
    st.get_mut(mgr_app).params = vec![mgr_res, mgr_ev];
    st.get_mut(mgr_app).paramss = vec![vec![mgr_res], vec![mgr_ev]];
    st.get_mut(mgr_app).ty = Type::Method {
        paramss: vec![
            vec![mr_t.clone()],
            vec![Type::Class {
                sym: releasable,
                args: vec![mr_t.clone()],
            }],
        ],
        ret: Box::new(mr_t),
    };
    st.get_mut(mgr_app).jvm_name =
        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)Ljava/lang/Object;".into();

    let mgr_acq = method(
        st,
        manager_cls,
        "acquire",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let acr = type_param(st, mgr_acq, "R");
    st.get_mut(mgr_acq).tparams = vec![acr];
    let acr_t = Type::TypeParam(acr);
    let acq_res = st.alloc(
        "resource",
        mgr_acq,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(acq_res).ty = acr_t.clone();
    let acq_ev = st.alloc(
        "releasable",
        mgr_acq,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(acq_ev).ty = Type::Class {
        sym: releasable,
        args: vec![acr_t.clone()],
    };
    st.get_mut(mgr_acq).params = vec![acq_res, acq_ev];
    st.get_mut(mgr_acq).paramss = vec![vec![acq_res], vec![acq_ev]];
    st.get_mut(mgr_acq).ty = Type::Method {
        paramss: vec![
            vec![acr_t.clone()],
            vec![Type::Class {
                sym: releasable,
                args: vec![acr_t],
            }],
        ],
        ret: Box::new(Type::Unit),
    };
    st.get_mut(mgr_acq).jvm_name = "(Ljava/lang/Object;Lscala/util/Using$Releasable;)V".into();

    let manager_mod = module(st, using_cls, "Manager", "scala/util/Using$Manager$");
    let manager_mcls = st.module_class_of(manager_mod);
    let mobj_app = method(
        st,
        manager_mcls,
        "apply",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let ma = type_param(st, mobj_app, "A");
    st.get_mut(mobj_app).tparams = vec![ma];
    let ma_t = Type::TypeParam(ma);
    let op = st.alloc(
        "op",
        mobj_app,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(op).ty = fn1(mgr_t, ma_t.clone());
    st.get_mut(mobj_app).params = vec![op];
    st.get_mut(mobj_app).paramss = vec![vec![op]];
    st.get_mut(mobj_app).ty = Type::Method {
        paramss: vec![vec![fn1(
            Type::Class {
                sym: manager_cls,
                args: vec![],
            },
            ma_t.clone(),
        )]],
        ret: Box::new(Type::Class {
            sym: try_c,
            args: vec![ma_t],
        }),
    };
    st.get_mut(mobj_app).jvm_name = "(Lscala/Function1;)Lscala/util/Try;".into();
    let mm_mems = st.get(manager_mcls).members.clone();
    st.get_mut(manager_mod).members.extend(mm_mems);

    // nsc `Using.resources` 2–4 resource overloads. First resource is by-value,
    // later ones by-name; result is `A` (throws, unlike `Using.apply`).
    add_using_resources(st, using_cls, releasable, 2);
    add_using_resources(st, using_cls, releasable, 3);
    add_using_resources(st, using_cls, releasable, 4);

    let mems = st.get(using_cls).members.clone();
    st.get_mut(using_mod).members.extend(mems);
}

/// nsc `Using.resources[R1, …, Rn, A](r1, r2: => …)(f)(implicit Releasable*)`.
///
/// JVM 2-arg: `(Object, Function0, Function2, Releasable, Releasable)Object`
/// and similarly Function3/Function4 for n=3/4.
fn add_using_resources(st: &mut SymbolTable, using_cls: SymbolId, releasable: SymbolId, n: usize) {
    let m = method(
        st,
        using_cls,
        "resources",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let mut rs = Vec::new();
    for i in 1..=n {
        rs.push(type_param(st, m, &format!("R{i}")));
    }
    let a = type_param(st, m, "A");
    let mut tps = rs.clone();
    tps.push(a);
    st.get_mut(m).tparams = tps;

    let mut p_ids = Vec::new();
    let mut p_tys = Vec::new();
    for (i, r) in rs.iter().enumerate() {
        let base = Type::TypeParam(*r);
        let ty = if i == 0 {
            base
        } else {
            Type::ByName(Box::new(base))
        };
        let p = st.alloc(
            &format!("resource{}", i + 1),
            m,
            crate::symbol::SymKind::Term,
            Flags::PARAM,
            "",
        );
        st.get_mut(p).ty = ty.clone();
        p_ids.push(p);
        p_tys.push(ty);
    }

    let fn_ty = fn_n(
        rs.iter().map(|r| Type::TypeParam(*r)).collect(),
        Type::TypeParam(a),
    );
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn_ty.clone();

    let mut ev_ids = Vec::new();
    let mut ev_tys = Vec::new();
    for (i, r) in rs.iter().enumerate() {
        let ev = st.alloc(
            &format!("evidence${}", i + 1),
            m,
            crate::symbol::SymKind::Term,
            Flags::PARAM.with(Flags::IMPLICIT),
            "",
        );
        let et = Type::Class {
            sym: releasable,
            args: vec![Type::TypeParam(*r)],
        };
        st.get_mut(ev).ty = et.clone();
        ev_ids.push(ev);
        ev_tys.push(et);
    }

    let mut all_params = p_ids.clone();
    all_params.push(f);
    all_params.extend(ev_ids.iter().copied());
    st.get_mut(m).params = all_params;
    st.get_mut(m).paramss = vec![p_ids, vec![f], ev_ids];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![p_tys, vec![fn_ty], ev_tys],
        ret: Box::new(Type::TypeParam(a)),
    };
    let mut desc = String::from("(Ljava/lang/Object;");
    for _ in 1..n {
        desc.push_str("Lscala/Function0;");
    }
    desc.push_str(&format!("Lscala/Function{n};"));
    for _ in 0..n {
        desc.push_str("Lscala/util/Using$Releasable;");
    }
    desc.push_str(")Ljava/lang/Object;");
    st.get_mut(m).jvm_name = desc;
}

fn add_rich_int_and_range(st: &mut SymbolTable) -> SymbolId {
    let range = class(
        st,
        st.scala_pkg,
        "Range",
        "scala/collection/immutable/Range",
        &[Type::AnyRef],
    );
    method(st, range, "length", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        range,
        "apply",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        range,
        "foreach",
        vec![fn1(Type::Int, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, range, "toString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        range,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    add_numeric_range(st);
    let ri = class(
        st,
        st.scala_pkg,
        "RichInt",
        "scala/runtime/RichInt",
        &[Type::AnyVal],
    );
    let f = st.alloc("self", ri, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::Int;
    st.get_mut(ri).ctor_fields = vec![f];
    method(st, ri, "abs", vec![], Type::Int, Intrinsic::None);
    method(st, ri, "max", vec![Type::Int], Type::Int, Intrinsic::None);
    method(st, ri, "min", vec![Type::Int], Type::Int, Intrinsic::None);
    let range_t = Type::Class {
        sym: range,
        args: vec![],
    };
    method(
        st,
        ri,
        "to",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(st, ri, "until", vec![Type::Int], range_t, Intrinsic::None);
    ri
}

fn add_rich_value(st: &mut SymbolTable, name: &str, jvm: &str, under: Type) -> SymbolId {
    let c = class(st, st.scala_pkg, name, jvm, &[Type::AnyVal]);
    let f = st.alloc("self", c, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = under.clone();
    st.get_mut(c).ctor_fields = vec![f];
    c
}

fn add_rich_long_double_char(st: &mut SymbolTable) -> (SymbolId, SymbolId, SymbolId) {
    let rl = add_rich_value(st, "RichLong", "scala/runtime/RichLong", Type::Long);
    method(st, rl, "abs", vec![], Type::Long, Intrinsic::None);
    method(st, rl, "max", vec![Type::Long], Type::Long, Intrinsic::None);
    method(st, rl, "min", vec![Type::Long], Type::Long, Intrinsic::None);
    let nr = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&id| st.get(id).name == "NumericRange")
        .expect("NumericRange");
    let nr_l = Type::Class {
        sym: nr,
        args: vec![Type::Long],
    };
    method(
        st,
        rl,
        "to",
        vec![Type::Long],
        nr_l.clone(),
        Intrinsic::None,
    );
    method(st, rl, "until", vec![Type::Long], nr_l, Intrinsic::None);
    let rd = add_rich_value(st, "RichDouble", "scala/runtime/RichDouble", Type::Double);
    method(st, rd, "abs", vec![], Type::Double, Intrinsic::None);
    method(
        st,
        rd,
        "max",
        vec![Type::Double],
        Type::Double,
        Intrinsic::None,
    );
    method(
        st,
        rd,
        "min",
        vec![Type::Double],
        Type::Double,
        Intrinsic::None,
    );
    let rc = add_rich_value(st, "RichChar", "scala/runtime/RichChar", Type::Char);
    method(st, rc, "isDigit", vec![], Type::Boolean, Intrinsic::None);
    method(st, rc, "toInt", vec![], Type::Int, Intrinsic::None);
    let nr_c = Type::Class {
        sym: nr,
        args: vec![Type::Char],
    };
    method(
        st,
        rc,
        "to",
        vec![Type::Char],
        nr_c.clone(),
        Intrinsic::None,
    );
    method(st, rc, "until", vec![Type::Char], nr_c, Intrinsic::None);
    (rl, rd, rc)
}

fn add_rich_float(st: &mut SymbolTable) -> SymbolId {
    let rf = add_rich_value(st, "RichFloat", "scala/runtime/RichFloat", Type::Float);
    method(st, rf, "abs", vec![], Type::Float, Intrinsic::None);
    method(
        st,
        rf,
        "max",
        vec![Type::Float],
        Type::Float,
        Intrinsic::None,
    );
    method(
        st,
        rf,
        "min",
        vec![Type::Float],
        Type::Float,
        Intrinsic::None,
    );
    rf
}

fn add_numeric_range(st: &mut SymbolTable) -> SymbolId {
    let nr = class(
        st,
        st.scala_pkg,
        "NumericRange",
        "scala/collection/immutable/NumericRange",
        &[Type::AnyRef],
    );
    let ta = type_param(st, nr, "T");
    st.get_mut(nr).tparams = vec![ta];
    let tt = Type::TypeParam(ta);
    method(
        st,
        nr,
        "foreach",
        vec![fn1(tt.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, nr, "toString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        nr,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, nr, "apply", vec![Type::Int], tt, Intrinsic::None);
    nr
}

fn add_rich_byte_short_boolean(st: &mut SymbolTable) -> (SymbolId, SymbolId, SymbolId) {
    let rb = add_rich_value(st, "RichByte", "scala/runtime/RichByte", Type::Byte);
    method(st, rb, "abs", vec![], Type::Byte, Intrinsic::None);
    method(st, rb, "max", vec![Type::Byte], Type::Byte, Intrinsic::None);
    method(st, rb, "min", vec![Type::Byte], Type::Byte, Intrinsic::None);
    let nr = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&id| st.get(id).name == "NumericRange")
        .expect("NumericRange");
    let nr_t = Type::Class {
        sym: nr,
        args: vec![Type::Byte],
    };
    method(
        st,
        rb,
        "to",
        vec![Type::Byte],
        nr_t.clone(),
        Intrinsic::None,
    );
    method(st, rb, "until", vec![Type::Byte], nr_t, Intrinsic::None);
    let rs = add_rich_value(st, "RichShort", "scala/runtime/RichShort", Type::Short);
    method(st, rs, "abs", vec![], Type::Short, Intrinsic::None);
    method(
        st,
        rs,
        "max",
        vec![Type::Short],
        Type::Short,
        Intrinsic::None,
    );
    method(
        st,
        rs,
        "min",
        vec![Type::Short],
        Type::Short,
        Intrinsic::None,
    );
    let nr_s = Type::Class {
        sym: nr,
        args: vec![Type::Short],
    };
    method(
        st,
        rs,
        "to",
        vec![Type::Short],
        nr_s.clone(),
        Intrinsic::None,
    );
    method(st, rs, "until", vec![Type::Short], nr_s, Intrinsic::None);
    let rbool = add_rich_value(
        st,
        "RichBoolean",
        "scala/runtime/RichBoolean",
        Type::Boolean,
    );
    method(
        st,
        rbool,
        "compare",
        vec![Type::Boolean],
        Type::Int,
        Intrinsic::None,
    );
    (rb, rs, rbool)
}

fn add_predef_members(
    st: &mut SymbolTable,
    arrow: SymbolId,
    string_ops: Option<SymbolId>,
    array_ops: Option<SymbolId>,
    rich_int: Option<SymbolId>,
    rich_ldc: Option<(SymbolId, SymbolId, SymbolId)>,
    library_abi: bool,
) {
    let p = st.predef;
    let cls = st.get(p).ty.clone();
    let owner = match cls {
        Type::ModuleRef(id) => id,
        _ => p,
    };
    // nsc `Predef.classOf[T]: Class[T]` — a class literal, not a real call.
    if let Some(jclass) = crate::classpath::find_by_jvm(st, "java/lang/Class") {
        let co = method(st, owner, "classOf", vec![], Type::Any, Intrinsic::ClassOf);
        let t = type_param(st, co, "T");
        st.get_mut(co).tparams = vec![t];
        st.get_mut(co).ty = Type::Method {
            paramss: Vec::new(),
            ret: Box::new(Type::Class {
                sym: jclass,
                args: vec![Type::TypeParam(t)],
            }),
        };
    }
    // `Any.getClass(): Class[_]`, inherited from `java.lang.Object`.
    if let Some(jclass) = crate::classpath::find_by_jvm(st, "java/lang/Class") {
        let any = st.any_sym;
        method(
            st,
            any,
            "getClass",
            vec![],
            Type::Class {
                sym: jclass,
                args: vec![Type::Any],
            },
            Intrinsic::GetClass,
        );
    }
    method(st, owner, "println", vec![], Type::Unit, Intrinsic::Println);
    method(
        st,
        owner,
        "println",
        vec![Type::Int],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Long],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Double],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::String],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Any],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "print",
        vec![Type::Any],
        Type::Unit,
        Intrinsic::Print,
    );
    method(
        st,
        owner,
        "assert",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Assert,
    );
    method(
        st,
        owner,
        "assert",
        vec![Type::Boolean, Type::ByName(Box::new(Type::Any))],
        Type::Unit,
        Intrinsic::Assert,
    );
    method(
        st,
        owner,
        "require",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Require,
    );
    method(
        st,
        owner,
        "require",
        vec![Type::Boolean, Type::ByName(Box::new(Type::Any))],
        Type::Unit,
        Intrinsic::Require,
    );
    method(
        st,
        owner,
        "???",
        vec![],
        Type::Nothing,
        Intrinsic::NotImplemented,
    );
    let ident = method(
        st,
        owner,
        "identity",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Identity,
    );
    let ia = type_param(st, ident, "A");
    st.get_mut(ident).tparams = vec![ia];
    st.get_mut(ident).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ia)]],
        ret: Box::new(Type::TypeParam(ia)),
    };
    let loc = method(
        st,
        owner,
        "locally",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Locally,
    );
    let lt = type_param(st, loc, "A");
    st.get_mut(loc).tparams = vec![lt];
    st.get_mut(loc).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(lt)]],
        ret: Box::new(Type::TypeParam(lt)),
    };
    let implm = method(
        st,
        owner,
        "implicitly",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Implicitly,
    );
    let it = type_param(st, implm, "T");
    let ip = st.alloc(
        "e",
        implm,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ip).ty = Type::TypeParam(it);
    st.get_mut(implm).tparams = vec![it];
    st.get_mut(implm).params = vec![ip];
    st.get_mut(implm).paramss = vec![vec![ip]];
    st.get_mut(implm).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(it)]],
        ret: Box::new(Type::TypeParam(it)),
    };
    let sadd = if library_abi {
        let s = class(
            st,
            st.scala_pkg,
            "any2stringadd",
            "scala/Predef$any2stringadd",
            &[Type::AnyVal],
        );
        let f = st.alloc("self", s, SymKind::Term, Flags::PARAM, "");
        st.get_mut(f).ty = Type::Any;
        st.get_mut(s).ctor_fields = vec![f];
        method(
            st,
            s,
            "+",
            vec![Type::String],
            Type::String,
            Intrinsic::None,
        );
        s
    } else {
        let s = class(
            st,
            st.scala_pkg,
            "any2stringadd",
            "scala/runtime/StringAdd",
            &[Type::AnyRef],
        );
        method(
            st,
            s,
            "+",
            vec![Type::String],
            Type::String,
            Intrinsic::StringConcat,
        );
        s
    };
    let conv_s = method(
        st,
        owner,
        "any2stringadd",
        vec![Type::Any],
        Type::Class {
            sym: sadd,
            args: vec![],
        },
        if library_abi {
            Intrinsic::Identity
        } else {
            Intrinsic::Any2StringAdd
        },
    );
    st.get_mut(conv_s).flags = st.get(conv_s).flags.with(Flags::IMPLICIT);
    let conv = method(
        st,
        owner,
        "any2ArrowAssoc",
        vec![Type::Any],
        Type::Class {
            sym: arrow,
            args: vec![],
        },
        Intrinsic::WrapArrowAssoc,
    );
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    if let Some(sops) = string_ops {
        let aug = method(
            st,
            owner,
            "augmentString",
            vec![Type::String],
            Type::Class {
                sym: sops,
                args: vec![],
            },
            Intrinsic::Identity,
        );
        st.get_mut(aug).flags = st.get(aug).flags.with(Flags::IMPLICIT);
        let seq = crate::classpath::find_or_stub_java_class(st, "scala/collection/Seq");
        let ws = class(
            st,
            st.scala_pkg,
            "WrappedString",
            "scala/collection/immutable/WrappedString",
            &[Type::Class {
                sym: seq,
                args: vec![Type::Char],
            }],
        );
        let wrap_str = method(
            st,
            owner,
            "wrapString",
            vec![Type::String],
            Type::Class {
                sym: ws,
                args: vec![],
            },
            Intrinsic::None,
        );
        st.get_mut(wrap_str).flags = st.get(wrap_str).flags.with(Flags::IMPLICIT);
        // javap: `wrapString` is declared on `scala.LowPriorityImplicits`,
        // `augmentString` on `Predef$`. So `StringOps` outranks
        // `WrappedString` whenever both offer the selected member -- which is
        // what `search_extension` already documents but could not act on
        // while this flag was unset (only `intWrapper` & co. carried it).
        st.get_mut(wrap_str).low_priority = true;
    }
    if let Some(aops) = array_ops {
        let wrap = method(
            st,
            owner,
            "intArrayOps",
            vec![Type::Array(Box::new(Type::Int))],
            Type::Class {
                sym: aops,
                args: vec![Type::Int],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
        let wrap_l = method(
            st,
            owner,
            "longArrayOps",
            vec![Type::Array(Box::new(Type::Long))],
            Type::Class {
                sym: aops,
                args: vec![Type::Long],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_l).flags = st.get(wrap_l).flags.with(Flags::IMPLICIT);
        let wrap_b = method(
            st,
            owner,
            "byteArrayOps",
            vec![Type::Array(Box::new(Type::Byte))],
            Type::Class {
                sym: aops,
                args: vec![Type::Byte],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_b).flags = st.get(wrap_b).flags.with(Flags::IMPLICIT);
        let wrap_s = method(
            st,
            owner,
            "shortArrayOps",
            vec![Type::Array(Box::new(Type::Short))],
            Type::Class {
                sym: aops,
                args: vec![Type::Short],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_s).flags = st.get(wrap_s).flags.with(Flags::IMPLICIT);
        let wrap_c = method(
            st,
            owner,
            "charArrayOps",
            vec![Type::Array(Box::new(Type::Char))],
            Type::Class {
                sym: aops,
                args: vec![Type::Char],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_c).flags = st.get(wrap_c).flags.with(Flags::IMPLICIT);
        let wrap_f = method(
            st,
            owner,
            "floatArrayOps",
            vec![Type::Array(Box::new(Type::Float))],
            Type::Class {
                sym: aops,
                args: vec![Type::Float],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_f).flags = st.get(wrap_f).flags.with(Flags::IMPLICIT);
        let wrap_d = method(
            st,
            owner,
            "doubleArrayOps",
            vec![Type::Array(Box::new(Type::Double))],
            Type::Class {
                sym: aops,
                args: vec![Type::Double],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_d).flags = st.get(wrap_d).flags.with(Flags::IMPLICIT);
        let wrap_bool = method(
            st,
            owner,
            "booleanArrayOps",
            vec![Type::Array(Box::new(Type::Boolean))],
            Type::Class {
                sym: aops,
                args: vec![Type::Boolean],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_bool).flags = st.get(wrap_bool).flags.with(Flags::IMPLICIT);
        let wrap_u = method(
            st,
            owner,
            "unitArrayOps",
            vec![Type::Array(Box::new(Type::Unit))],
            Type::Class {
                sym: aops,
                args: vec![Type::Unit],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_u).flags = st.get(wrap_u).flags.with(Flags::IMPLICIT);
        let wrap_ref = method(
            st,
            owner,
            "refArrayOps",
            vec![Type::Array(Box::new(Type::AnyRef))],
            Type::Class {
                sym: aops,
                args: vec![Type::AnyRef],
            },
            Intrinsic::Identity,
        );
        let rt = type_param(st, wrap_ref, "T");
        st.get_mut(wrap_ref).tparams = vec![rt];
        st.get_mut(wrap_ref).ty = Type::Method {
            paramss: vec![vec![Type::Array(Box::new(Type::TypeParam(rt)))]],
            ret: Box::new(Type::Class {
                sym: aops,
                args: vec![Type::TypeParam(rt)],
            }),
        };
        st.get_mut(wrap_ref).flags = st.get(wrap_ref).flags.with(Flags::IMPLICIT);
        // nsc Predef.genericArrayOps[T](xs: Array[T]): ArrayOps[T] — the only
        // conversion that applies to an unconstrained type parameter `Array[T]`
        // (refArrayOps requires T <: AnyRef; primitive wrappers need Array[Int] etc.).
        let wrap_g = method(
            st,
            owner,
            "genericArrayOps",
            vec![Type::Array(Box::new(Type::Any))],
            Type::Class {
                sym: aops,
                args: vec![Type::Any],
            },
            Intrinsic::Identity,
        );
        let gt = type_param(st, wrap_g, "T");
        st.get_mut(wrap_g).tparams = vec![gt];
        st.get_mut(wrap_g).ty = Type::Method {
            paramss: vec![vec![Type::Array(Box::new(Type::TypeParam(gt)))]],
            ret: Box::new(Type::Class {
                sym: aops,
                args: vec![Type::TypeParam(gt)],
            }),
        };
        st.get_mut(wrap_g).flags = st.get(wrap_g).flags.with(Flags::IMPLICIT);
    }
    if let Some(ri) = rich_int {
        let wrap = method(
            st,
            owner,
            "intWrapper",
            vec![Type::Int],
            Type::Class {
                sym: ri,
                args: vec![],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
    }
    if let Some((rl, rd, rc)) = rich_ldc {
        add_numeric_wrapper(st, owner, "longWrapper", Type::Long, rl);
        add_numeric_wrapper(st, owner, "doubleWrapper", Type::Double, rd);
        add_numeric_wrapper(st, owner, "charWrapper", Type::Char, rc);
    }
    if library_abi {
        let rf = add_rich_float(st);
        add_numeric_wrapper(st, owner, "floatWrapper", Type::Float, rf);
        let (rb, rs, rbool) = add_rich_byte_short_boolean(st);
        add_numeric_wrapper(st, owner, "byteWrapper", Type::Byte, rb);
        add_numeric_wrapper(st, owner, "shortWrapper", Type::Short, rs);
        add_numeric_wrapper(st, owner, "booleanWrapper", Type::Boolean, rbool);
        let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
        if let Some(of_int) = st
            .lookup_member(mutp, "ArraySeq$ofInt")
            .into_iter()
            .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        {
            // nsc LowPriorityImplicits.wrapIntArray — not IMPLICIT here so it
            // does not compete with intArrayOps for Array members.
            method(
                st,
                owner,
                "wrapIntArray",
                vec![Type::Array(Box::new(Type::Int))],
                Type::Class {
                    sym: of_int,
                    args: vec![],
                },
                Intrinsic::None,
            );
        }
    }
    let mems = st.get(owner).members.clone();
    st.get_mut(p).members.extend(mems.iter().copied());
    for m in mems {
        let name = st.get(m).name.clone();
        st.enter_in_current(&name, m);
    }
}

fn add_numeric_wrapper(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    from: Type,
    cls: SymbolId,
) {
    let wrap = method(
        st,
        owner,
        name,
        vec![from],
        Type::Class {
            sym: cls,
            args: vec![],
        },
        Intrinsic::Identity,
    );
    st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
    // nsc declares `intWrapper` & co. in `LowPriorityImplicits`, which `Predef`
    // extends, so `Predef`'s own `double2Double` outranks `doubleWrapper` when
    // both results offer the selected member (`0.5.isNaN`).
    st.get_mut(wrap).low_priority = true;
}

/// A parameterless getter that is *not* an implicit candidate.
fn plain_getter(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) {
    let id = st.alloc(name, owner, SymKind::Method, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::Method {
        paramss: vec![],
        ret: Box::new(ty),
    };
}

fn implicit_getter(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::IMPLICIT, "");
    st.get_mut(id).ty = Type::Method {
        paramss: vec![],
        ret: Box::new(ty),
    };
    id
}

fn add_classtag(st: &mut SymbolTable, jclass: SymbolId) -> SymbolId {
    let reflect = st.alloc(
        "reflect",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/reflect",
    );
    let ct = iface(st, reflect, "ClassTag", "scala/reflect/ClassTag");
    let t = type_param(st, ct, "T");
    st.get_mut(ct).tparams = vec![t];
    let class_ty = Type::Class {
        sym: jclass,
        args: vec![],
    };
    method(
        st,
        ct,
        "runtimeClass",
        vec![],
        class_ty.clone(),
        Intrinsic::None,
    );
    method(
        st,
        ct,
        "newArray",
        vec![Type::Int],
        Type::Array(Box::new(Type::TypeParam(t))),
        Intrinsic::None,
    );
    let ctm = module(st, reflect, "ClassTag", "scala/reflect/ClassTag$");
    let mc = st.module_class_of(ctm);
    let tag = |elem: Type| Type::Class {
        sym: ct,
        args: vec![elem],
    };
    implicit_getter(st, mc, "Int", tag(Type::Int));
    implicit_getter(st, mc, "Long", tag(Type::Long));
    implicit_getter(st, mc, "Double", tag(Type::Double));
    implicit_getter(st, mc, "Float", tag(Type::Float));
    implicit_getter(st, mc, "Boolean", tag(Type::Boolean));
    implicit_getter(st, mc, "Byte", tag(Type::Byte));
    implicit_getter(st, mc, "Short", tag(Type::Short));
    implicit_getter(st, mc, "Char", tag(Type::Char));
    implicit_getter(st, mc, "Unit", tag(Type::Unit));
    implicit_getter(st, mc, "Any", tag(Type::Any));
    implicit_getter(st, mc, "AnyRef", tag(Type::AnyRef));
    // `scala.AnyRef` is an alias of `java.lang.Object`, so `ClassTag.Object`
    // has the very type `ClassTag.AnyRef` has. Only one of the two may be a
    // candidate or `Array("x", "y"): Array[AnyRef]` is ambiguous -- in nsc
    // neither is implicit at all (the compiler materializes class tags).
    plain_getter(st, mc, "Object", tag(Type::AnyRef));
    implicit_getter(st, mc, "Nothing", tag(Type::Nothing));
    implicit_getter(st, mc, "Null", tag(Type::Null));
    let apply = method(
        st,
        mc,
        "apply",
        vec![class_ty.clone()],
        tag(Type::Any),
        Intrinsic::None,
    );
    let at = type_param(st, apply, "T");
    st.get_mut(apply).tparams = vec![at];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![vec![class_ty]],
        ret: Box::new(tag(Type::TypeParam(at))),
    };
    let mems = st.get(mc).members.clone();
    st.get_mut(ctm).members.extend(mems);
    ct
}

/// `StringContext.parts` is a `Seq[String]`; `Seq` only exists once
/// `add_seq_and_lazylist` has run, so the type is filled in afterwards.
fn fix_string_context_parts(st: &mut SymbolTable) {
    let Some(seq) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq") else {
        return;
    };
    let Some(sc) = crate::classpath::find_by_jvm(st, "scala/StringContext") else {
        return;
    };
    let fields = st.get(sc).ctor_fields.clone();
    for f in fields {
        if st.get(f).name == "parts" {
            st.get_mut(f).ty = Type::Class {
                sym: seq,
                args: vec![Type::String],
            };
        }
    }
}

fn add_string_context(st: &mut SymbolTable) {
    let sc = class(
        st,
        st.scala_pkg,
        "StringContext",
        "scala/StringContext",
        &[Type::AnyRef],
    );
    // `new StringContext(parts: String*)` takes a repeated parameter, but the
    // member `parts` is a `Seq[String]`.
    let parts = st.alloc("parts", sc, SymKind::Term, Flags::PARAM, "parts");
    let seq = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq");
    st.get_mut(parts).ty = match seq {
        Some(seq) => Type::Class {
            sym: seq,
            args: vec![Type::String],
        },
        None => Type::Repeated(Box::new(Type::String)),
    };
    st.get_mut(sc).ctor_fields = vec![parts];
    method(
        st,
        sc,
        "s",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::String,
        Intrinsic::None,
    );
    let scm = module(st, st.scala_pkg, "StringContext", "scala/StringContext$");
    let mc = st.module_class_of(scm);
    method(
        st,
        mc,
        "apply",
        vec![Type::Repeated(Box::new(Type::String))],
        Type::Class {
            sym: sc,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(mc).members.clone();
    st.get_mut(scm).members.extend(mems);
}

/// `scala.Array` companion from scala-library. Do not emit `Array$.class`.
fn add_array_companion(st: &mut SymbolTable, ct: SymbolId) {
    let am = module(st, st.scala_pkg, "Array", "scala/Array$");
    let mc = st.module_class_of(am);
    let apply = method(
        st,
        mc,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::Array(Box::new(Type::Any)),
        Intrinsic::None,
    );
    let t = type_param(st, apply, "T");
    let xs = st.alloc("xs", apply, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(xs).ty = Type::Repeated(Box::new(Type::TypeParam(t)));
    let ev = st.alloc(
        "evidence$1",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(t)],
    };
    st.get_mut(apply).tparams = vec![t];
    st.get_mut(apply).params = vec![xs, ev];
    st.get_mut(apply).paramss = vec![vec![xs], vec![ev]];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![
            vec![Type::Repeated(Box::new(Type::TypeParam(t)))],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(t)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(t)))),
    };
    let mems = st.get(mc).members.clone();
    st.get_mut(am).members.extend(mems);
}

fn ctor_field(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Term, Flags::PARAM, "");
    st.get_mut(id).ty = ty;
    id
}

fn abs_class(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parents: &[Type],
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Class, Flags::ABSTRACT, jvm);
    st.get_mut(id).parents = parents.to_vec();
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}

/// scala-xml 2.3 (`Elem(String, String, MetaData, NamespaceBinding, boolean, Seq[Node])`).
fn add_xml(st: &mut SymbolTable) {
    let xml = st.alloc(
        "xml",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/xml",
    );
    let node = abs_class(st, xml, "Node", "scala/xml/Node", &[Type::AnyRef]);
    let node_t = Type::Class {
        sym: node,
        args: vec![],
    };
    let metadata = abs_class(st, xml, "MetaData", "scala/xml/MetaData", &[Type::AnyRef]);
    let nsb = abs_class(
        st,
        xml,
        "NamespaceBinding",
        "scala/xml/NamespaceBinding",
        &[Type::AnyRef],
    );
    let _null = module_extending(
        st,
        xml,
        "Null",
        "scala/xml/Null$",
        Type::Class {
            sym: metadata,
            args: vec![],
        },
    );
    let _top = module_extending(
        st,
        xml,
        "TopScope",
        "scala/xml/TopScope$",
        Type::Class {
            sym: nsb,
            args: vec![],
        },
    );
    let seq = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).name == "Seq" && st.get(m).kind == SymKind::Class)
        .expect("Seq");
    let seq_node = Type::Class {
        sym: seq,
        args: vec![node_t.clone()],
    };
    let elem = class(st, xml, "Elem", "scala/xml/Elem", &[node_t.clone()]);
    let p_prefix = ctor_field(st, elem, "prefix", Type::String);
    let p_label = ctor_field(st, elem, "label", Type::String);
    let p_attr = ctor_field(
        st,
        elem,
        "attributes",
        Type::Class {
            sym: metadata,
            args: vec![],
        },
    );
    let p_scope = ctor_field(
        st,
        elem,
        "scope",
        Type::Class {
            sym: nsb,
            args: vec![],
        },
    );
    let p_min = ctor_field(st, elem, "minimizeEmpty", Type::Boolean);
    let p_child = ctor_field(st, elem, "child", seq_node);
    st.get_mut(elem).ctor_fields = vec![p_prefix, p_label, p_attr, p_scope, p_min, p_child];
    let text = class(st, xml, "Text", "scala/xml/Text", &[node_t.clone()]);
    let td = ctor_field(st, text, "data", Type::String);
    st.get_mut(text).ctor_fields = vec![td];
    let eref = class(
        st,
        xml,
        "EntityRef",
        "scala/xml/EntityRef",
        &[node_t.clone()],
    );
    let en = ctor_field(st, eref, "entityName", Type::String);
    st.get_mut(eref).ctor_fields = vec![en];
    let comment = class(st, xml, "Comment", "scala/xml/Comment", &[node_t.clone()]);
    let ct = ctor_field(st, comment, "commentText", Type::String);
    st.get_mut(comment).ctor_fields = vec![ct];
    let pcdata = class(st, xml, "PCData", "scala/xml/PCData", &[node_t.clone()]);
    let pd = ctor_field(st, pcdata, "data", Type::String);
    st.get_mut(pcdata).ctor_fields = vec![pd];
    let pi = class(
        st,
        xml,
        "ProcInstr",
        "scala/xml/ProcInstr",
        &[node_t.clone()],
    );
    let pit = ctor_field(st, pi, "target", Type::String);
    let pip = ctor_field(st, pi, "proctext", Type::String);
    st.get_mut(pi).ctor_fields = vec![pit, pip];
    let atom = class(st, xml, "Atom", "scala/xml/Atom", &[node_t]);
    let ad = ctor_field(st, atom, "data", Type::Any);
    st.get_mut(atom).ctor_fields = vec![ad];
    let meta_t = Type::Class {
        sym: metadata,
        args: vec![],
    };
    let upa = class(
        st,
        xml,
        "UnprefixedAttribute",
        "scala/xml/UnprefixedAttribute",
        &[meta_t.clone()],
    );
    let uk = ctor_field(st, upa, "key", Type::String);
    let uv = ctor_field(st, upa, "value", Type::String);
    let un = ctor_field(st, upa, "next", meta_t.clone());
    st.get_mut(upa).ctor_fields = vec![uk, uv, un];
    let nsb_t = Type::Class {
        sym: nsb,
        args: vec![],
    };
    let np = ctor_field(st, nsb, "prefix", Type::String);
    let nu = ctor_field(st, nsb, "uri", Type::String);
    let npar = ctor_field(st, nsb, "parent", nsb_t);
    st.get_mut(nsb).ctor_fields = vec![np, nu, npar];
    let pa = class(
        st,
        xml,
        "PrefixedAttribute",
        "scala/xml/PrefixedAttribute",
        &[meta_t.clone()],
    );
    let pp = ctor_field(st, pa, "pre", Type::String);
    let pk = ctor_field(st, pa, "key", Type::String);
    let pv = ctor_field(st, pa, "value", Type::String);
    let pn = ctor_field(st, pa, "next", meta_t);
    st.get_mut(pa).ctor_fields = vec![pp, pk, pv, pn];
}

/// `scala.Enumeration` plus inner `Value` (`Color.Red.toString` / `.id` against the jar).
fn add_enumeration(st: &mut SymbolTable) {
    let en = abs_class(
        st,
        st.scala_pkg,
        "Enumeration",
        "scala/Enumeration",
        &[Type::AnyRef],
    );
    let val = abs_class(st, en, "Value", "scala/Enumeration$Value", &[Type::AnyRef]);
    method(st, val, "id", vec![], Type::Int, Intrinsic::None);
    let val_t = Type::Class {
        sym: val,
        args: vec![],
    };
    method(st, en, "Value", vec![], val_t, Intrinsic::None);
}

/// `scala.DelayedInit` / `scala.App` (nsc delayed constructor body).
fn add_delayed_init_app(st: &mut SymbolTable) {
    let di = iface(st, st.scala_pkg, "DelayedInit", "scala/DelayedInit");
    let d = st.alloc("delayedInit", di, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(d).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::Unit))]],
        ret: Box::new(Type::Unit),
    };
    let p = st.alloc("x", d, SymKind::Term, Flags::PARAM.with(Flags::BYNAME), "");
    st.get_mut(p).ty = Type::ByName(Box::new(Type::Unit));
    st.get_mut(d).params = vec![p];
    st.get_mut(d).paramss = vec![vec![p]];

    let app = iface(st, st.scala_pkg, "App", "scala/App");
    st.get_mut(app).parents = vec![
        Type::Class {
            sym: di,
            args: vec![],
        },
        Type::AnyRef,
    ];
    let d2 = st.alloc("delayedInit", app, SymKind::Method, Flags::EMPTY, "");
    st.get_mut(d2).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::Unit))]],
        ret: Box::new(Type::Unit),
    };
    let p2 = st.alloc("x", d2, SymKind::Term, Flags::PARAM.with(Flags::BYNAME), "");
    st.get_mut(p2).ty = Type::ByName(Box::new(Type::Unit));
    st.get_mut(d2).params = vec![p2];
    st.get_mut(d2).paramss = vec![vec![p2]];

    let main = st.alloc("main", app, SymKind::Method, Flags::EMPTY, "");
    let args_ty = Type::Array(Box::new(Type::String));
    st.get_mut(main).ty = Type::Method {
        paramss: vec![vec![args_ty.clone()]],
        ret: Box::new(Type::Unit),
    };
    let ap = st.alloc("args", main, SymKind::Term, Flags::PARAM, "");
    st.get_mut(ap).ty = args_ty;
    st.get_mut(main).params = vec![ap];
    st.get_mut(main).paramss = vec![vec![ap]];
}
