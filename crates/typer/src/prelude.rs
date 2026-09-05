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
    // `::` is the source spelling of `$colon$colon`, not a class of its own.
    // It used to be a second `SymKind::Class` symbol with the same JVM name
    // and nothing else on it -- no type parameter, no constructor fields, no
    // parents -- and `import_members` entered it under `::` ahead of
    // `st.enter_in_current("::", st.cons_sym)` below, so it was what the name
    // resolved to: `val c: ::[Int]` was ":: does not take type parameters"
    // and `new ::(1, Nil)` was "no matching overload for constructor ::".
    // The scope entry below is the whole alias.

    crate::prelude_anyval2::add_any_members(st);
    crate::prelude_anyval2::add_int_members(st);
    crate::prelude_anyval2::add_long_members(st);
    crate::prelude_anyval2::add_double_members(st);
    crate::prelude_anyval2::add_float_members(st);
    crate::prelude_anyval2::add_bool_members(st);
    crate::prelude_numeric::install(st, library_abi);
    crate::prelude_anyval2::add_string_members(st, library_abi);
    crate::prelude_anyval2::add_array_members(st);
    let with_filter = crate::prelude_iterprim::add_with_filter(st);
    let option_wf = crate::prelude_iterprim::add_option_with_filter(st);
    let iterator = if library_abi {
        Some(crate::prelude_iterprim::add_iterator(st))
    } else {
        None
    };
    let string_ops = if library_abi {
        Some(crate::prelude_stringops_core::add_string_ops(
            st,
            iterator.unwrap(),
        ))
    } else {
        None
    };
    let array_ops = if library_abi {
        Some(crate::prelude_arrayops2::add_array_ops(st))
    } else {
        None
    };
    crate::prelude_immutcoll2::add_option_members(st, option_wf, library_abi);
    crate::prelude_sgap::fix_option_flat_map(st);
    crate::prelude_immutcoll2::add_cons_members(st, library_abi);
    crate::prelude_either::install_option_core(st);
    crate::prelude_either::install_java_lang_exceptions(st);
    crate::prelude_immutcoll2::add_list_members(st, with_filter, iterator, library_abi);
    crate::prelude_immutcoll2::add_function_types(st);
    crate::prelude_immutcoll2::add_partial_function(st);
    if library_abi {
        crate::prelude_immutcoll2::add_list_collect(st);
        let ct = crate::prelude_classtag2::add_classtag(st, jclass);
        if let Some(aops) = array_ops {
            crate::prelude_arrayops2::add_array_ops_map(st, aops, ct);
            crate::prelude_arrayops2::add_array_ops_flat_map(st, aops, ct);
            crate::prelude_arrayops2::add_array_ops_flat_map_from_array(st, aops, ct);
            crate::prelude_arrayops2::add_array_ops_collect(st, aops, ct);
        }
        if let Some(so) = string_ops {
            crate::prelude_stringops_core::add_string_ops_to_array(st, so, ct);
        }
        crate::prelude_stringops_core::add_string_context(st);
        crate::prelude_arrayops2::add_array_companion(st, ct);
    }
    let ordered = crate::prelude_ordering2::add_ordered(st);
    crate::prelude_xmlenum::add_delayed_init_app(st);

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
            crate::prelude_arrayops2::add_array_ops_zip(st, aops, tuple2);
            crate::prelude_arrayops2::add_array_ops_folds(st, aops);
            crate::prelude_arrayops2::add_array_ops_scan_left(st, aops);
        }
        if let Some(so) = string_ops {
            crate::prelude_stringops_core::add_string_ops_fold_left(st, so);
            crate::prelude_stringops_core::add_string_ops_fold_right_and_grouped(st, so);
            crate::prelude_stringops_core::add_string_ops_map_and_appended(st, so);
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
        Some(crate::prelude_richnum::add_rich_int_and_range(st))
    } else {
        None
    };
    let rich_ldc = if library_abi {
        Some(crate::prelude_richnum::add_rich_long_double_char(st))
    } else {
        None
    };
    if library_abi {
        crate::prelude_immutcoll2::add_map_and_vector(st);
        crate::prelude_immutcoll2::add_set(st);
        let ordering = crate::prelude_ordering2::add_ordering(st);
        crate::prelude_ordering2::add_sorted_set(st, ordering);
        crate::prelude_ordering2::add_sorted_map(st, ordering);
        crate::prelude_immutcoll2::add_bit_set(st);
        if let Some(so) = string_ops {
            crate::prelude_stringops_core::add_string_ops_sorted(st, so, ordering);
            crate::prelude_stringops_core::add_string_ops_indices_and_r(st, so);
            crate::prelude_stringops_core::add_string_ops_compare_patch_length(st, so);
            if let Some(it) = iterator {
                crate::prelude_stringops_core::add_string_ops_iterator_size_appended(st, so, it);
            }
        }
        if let Some(aops) = array_ops {
            crate::prelude_arrayops2::add_array_ops_remaining(st, aops);
            crate::prelude_arrayops2::add_array_ops_filter_not_opts_part(st, aops, tuple2);
            crate::prelude_arrayops2::add_array_ops_zip_index_size(st, aops, tuple2);
            if let Some(it) = iterator {
                crate::prelude_arrayops2::add_array_ops_length_index_copy(st, aops, it);
            }
        }
        if let Some(so) = string_ops {
            crate::prelude_stringops_core::add_string_ops_concat_length_flat(st, so);
        }
        crate::prelude_immutcoll2::add_seq_and_lazylist(st);
        crate::prelude_stringops_core::fix_string_context_parts(st);
        crate::prelude_immutcoll2::add_view(st);
        crate::prelude_immutcoll2::add_indexedseq_and_queue(st);
        crate::prelude_mutcoll2::add_array_buffer(st);
        crate::prelude_mutcoll2::add_list_buffer(st);
        crate::prelude_mutcoll2::add_array_deque(st);
        crate::prelude_mutcoll2::add_string_builder(st);
        crate::prelude_mutcoll2::add_hash_map(st);
        crate::prelude_mutcoll2::add_hash_set(st);
        if let Some(it) = iterator {
            crate::prelude_coll::add_collections_extra(st, tuple2, ordering, it);
        }
        crate::prelude_sgap::add_iterable_apply(st, library_abi);
        if let Some(aops) = array_ops {
            // ArrayOps' conversions and aggregates (toList/toSeq/groupBy/sum/...) and
            // scala.collection.MapView. Run after the collections themselves so that
            // Buffer / Iterable / MapView are not rebuilt.
            crate::prelude_arrconv::install(st, aops, tuple2, ordering);
        }
        crate::prelude_mutcoll2::add_linked_hash_map(st);
        crate::prelude_mutcoll2::add_linked_hash_set(st);
        crate::prelude_eithertry::add_either(st);
        crate::prelude_eithertry::add_try(st, throwable);
        crate::prelude_either::install_library_abi(st);
        crate::prelude_eithertry::add_breaks(st);
        crate::prelude_bignum::add_big_int(st);
        crate::prelude_bignum::add_big_decimal(st);
        crate::prelude_oshadow::install(st);
        crate::prelude_mism12::install(st);
        crate::prelude_chainusing::add_chaining(st);
        crate::prelude_chainusing::add_using(st);
        crate::prelude_xmlenum::add_xml(st);
        crate::prelude_xmlenum::add_enumeration(st);
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
    crate::prelude_predef2::add_predef_members(
        st,
        arrow,
        string_ops,
        array_ops,
        rich_int,
        rich_ldc,
        library_abi,
    );

    crate::prelude_lowbound::install(st);
    // The `[B >: A]` of `Option.getOrElse` / `orElse` / `Map.getOrElse`.
    crate::prelude_ovl3::install(st, library_abi);
    // The `[B1 >: B]` of `Either.getOrElse` / `Try.getOrElse` (`add_either` /
    // `add_try` wrote them as `(=> Any): Any`).
    crate::prelude_dbio::install(st);
    crate::prelude_lang::install(st);
    crate::prelude_lazyref::install(st);

    st.push_scope();
    // Everything below lands in this scope, and it stays open for the whole
    // run: it is what `java.lang._` / `scala._` / `Predef._` being open
    // around every unit amounts to here.
    st.prelude_scope = st.scopes.len() - 1;
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
    crate::prelude_classtag::install(st, library_abi);
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
    // The remaining edges, such as `HashSet <: mutable.Set`. After `prelude_hier` has
    // assembled the `collection.Set` / `collection.Map` side.
    crate::prelude_ovl3::install_hierarchy(st);
    // `get`/`contains`/`getOrElse`/`apply` on `collection.Map`.
    // After `prelude_hier` has built the linking traits.
    crate::prelude_implfind::install(st);
    // `Seq[A] <: PartialFunction[Int, A] <: Int => A` (after `prelude_hier`, since it
    // is what assembles `scala/collection/Seq`). `PartialFunction`'s `lift`/`orElse`
    // go in on the same slice. `library_abi` only.
    crate::prelude_seqfn::install(st, library_abi);
    // The `Predef` wrapping that lets an `Array` be passed as a `Seq`/`Iterable`, and
    // the read members of `collection.Map` (agent/setmap). After `prelude_hier` has
    // assembled `collection/Map` and `prelude_seqfn` has assembled the
    // `mutable.ArraySeq` area.
    crate::prelude_setmap::install(st, library_abi);
    crate::prelude_fntuple::install(st, library_abi);
    crate::prelude_mism9::install(st);
    if library_abi {
        crate::prelude_durrange::install_range_companion(st);
        crate::prelude_durrange::install_ordered_companion(st);
    }
    // `val Ordering = scala.math.Ordering` (the term-position alias). Last, so that it
    // goes in once all the companions are present.
    crate::prelude_ordsummon::install(st, library_abi);
    // `Ordering[T] <: PartialOrdering[T] <: Equiv[T]` and `object Equiv`'s implicit
    // instances. After the line above has installed `Equiv`'s companion alias.
    crate::prelude_eqtail::install(st, library_abi);
    // `3.compare(4)`: give `RichInt` and friends a `compare`. Without it the
    // search falls through to the `Ordered.orderingToOrdered` view, whose
    // conversion never got materialised -- a `checkcast scala/math/Ordered`
    // landed on an `int`.
    crate::prelude_richcmp::install_rich_compare(st);
    // Flags the hand-written declarations above leave off that the library's
    // pickle carries. Last, so every class it names already exists.
    crate::prelude_fidelity::install(st);
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
    // `Map[K, V] <: K => V`. Wired up after the hierarchy table.
    crate::prelude_mism4::install(st);
    // `case Seq(a, b)` / `case Array(a, b)`. Added once the companions are present.
    crate::prelude_seqpat::install(st);
    // `case h +: t` / `case t :+ h` / `case h #:: t`. Needs `immutable.Seq`,
    // `LazyList` and `Stream`, which the hierarchy above has by now built.
    crate::prelude_consextract::install(st, library_abi);
    // What `SeqView.filter` and friends return is a `View`, not a `SeqView`.
    crate::prelude_viewc::install(st);
    // `StringOps.map[B](Char => B): IndexedSeq[B]` (once `IndexedSeq` is present).
    crate::prelude_strmap::install(st, library_abi);
    // The rest of `StringOps`. Only what cannot be completed from the pickle (such as
    // overloads differing solely in return type) is hand-written.
    crate::prelude_stringops8::install(st, library_abi);
    // `Coll.empty` is made polymorphic in one final pass (once every companion is present).
    crate::prelude_empty::install(st);
    // `java.lang.ClassLoader` and `Class#getClassLoader()`, so
    // `JavaUniverse#runtimeMirror(ClassLoader)` -- read from scala-reflect.jar's
    // own pickle -- has a parameter type to install against. Unconditional:
    // `ClassLoader` is a plain JDK class, not something `--scala-library` gates.
    crate::prelude_reflectruntime::install(st);
}

pub(crate) fn mark_java(st: &mut SymbolTable, id: SymbolId) {
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
    // `abstract class Annotation` has a public no-argument constructor, and it
    // is the superclass every annotation class gets (`class a extends
    // StaticAnnotation` compiles to `extends Annotation implements
    // StaticAnnotation`), so the subclass's `<init>` has one to call.
    method(
        st,
        annotation,
        "<init>",
        vec![],
        Type::Class {
            sym: annotation,
            args: vec![],
        },
        Intrinsic::None,
    );
    // `trait StaticAnnotation extends Annotation`: the class file really is
    // `public interface scala.annotation.StaticAnnotation`, and nsc compiles
    // `class a extends Annotation with StaticAnnotation` (cats-kernel's
    // `suppressUnusedImportWarningForScalaVersionSpecific`) to `extends
    // Annotation implements StaticAnnotation`. Declared as a class here, that
    // source was rejected: `class StaticAnnotation needs to be a trait to be
    // mixed in`.
    let static_annot = iface(
        st,
        pkg,
        "StaticAnnotation",
        "scala/annotation/StaticAnnotation",
    );
    st.get_mut(static_annot).parents = vec![Type::Class {
        sym: annotation,
        args: vec![],
    }];
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

pub(crate) fn ctor_field(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Term, Flags::PARAM, "");
    st.get_mut(id).ty = ty;
    id
}

pub(crate) fn abs_class(
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
