//! `scala.math` の型クラス階層（`Numeric` / `Integral` / `Fractional` /
//! `Ordering`）の親子関係と、`object Numeric` の implicit インスタンスの型。
//!
//! `crates/typer/src/prelude.rs` の `install_prelude` から 1 行だけ呼ばれる。
//! マージ衝突を避けるため新規モジュールに分けた（`.agent-brief.md` の方針）。
//!
//! prelude は `scala.math.Numeric` を「`sum` / `product` の implicit 引数を
//! 解決するための入れ物」として合成しているだけで、実 ABI の
//! `interface scala.math.Numeric<T> extends scala.math.Ordering<T>`
//! （`javap` で確認）という継承を写していなかった。そのため
//!
//! ```scala
//! class B[T](implicit ct: ClassTag[T], ord: Ordering[T])
//! class N[T](implicit tag: ClassTag[T], num: Numeric[T]) extends B[T]()(tag, num)
//! ```
//!
//! （slick `ScalaNumericType`）の `Numeric[T]` → `Ordering[T]` が通らず、
//! `no matching overload for constructor` になっていた。メンバ解決だけは
//! `lookup_member` が別経路で当たっていたので、症状は「サブタイプだけ落ちる」
//! という分かりにくい形で出る。
//!
//! # `Integral` / `Fractional`（`agent/integral`）
//!
//! `List.range(0, 5)` / `Vector.range` / `Seq.range` は
//! `IterableFactory#range[A](start: A, end: A)(implicit ord: Integral[A])` で、
//! `no implicit: could not find implicit value of type Integral[Int]` に
//! なっていた。原因は 2 つ:
//!
//! 1. `Integral` / `Fractional` は prelude の時点では symbol table にいない。
//!    ソースが名前を出すと `pickle_supply` がスタブを起こし、**メンバ解決に
//!    失敗したときだけ** pickle から親（`Numeric`）を後付けする。つまり
//!    subtyping の判定には間に合わず、`Integral[T] <: Numeric[T]` の辺が
//!    無いままだった。
//! 2. `object Numeric` の implicit インスタンスに `Numeric[Int]` を付けて
//!    いた。実 ABI は `Numeric$IntIsIntegral$` が
//!    `Numeric$IntIsIntegral extends Integral<Object>` を実装しており、
//!    `Integral[Int]` が正しい。
//!
//! `javap -p -s /tmp/scala-rs-lib/scala-library-2.13.16.jar` で確かめた形:
//!
//! ```text
//! interface scala.math.Numeric<T>    extends scala.math.Ordering<T>
//! interface scala.math.Integral<T>   extends scala.math.Numeric<T>
//! interface scala.math.Fractional<T> extends scala.math.Numeric<T>
//!
//! Numeric$IntIsIntegral$        implements Numeric$IntIsIntegral,        Ordering$IntOrdering
//! Numeric$LongIsIntegral$       implements Numeric$LongIsIntegral,       Ordering$LongOrdering
//! Numeric$ByteIsIntegral$       implements Numeric$ByteIsIntegral,       Ordering$ByteOrdering
//! Numeric$ShortIsIntegral$      implements Numeric$ShortIsIntegral,      Ordering$ShortOrdering
//! Numeric$CharIsIntegral$       implements Numeric$CharIsIntegral,       Ordering$CharOrdering
//! Numeric$BigIntIsIntegral$     implements Numeric$BigIntIsIntegral,     Ordering$BigIntOrdering
//! Numeric$DoubleIsFractional$   implements Numeric$DoubleIsFractional,   Ordering$Double$IeeeOrdering
//! Numeric$FloatIsFractional$    implements Numeric$FloatIsFractional,    Ordering$Float$IeeeOrdering
//! Numeric$BigDecimalIsFractional$ implements Numeric$BigDecimalIsFractional, Ordering$BigDecimalOrdering
//!
//! interface Numeric$IntIsIntegral          extends Integral<Object>
//! interface Numeric$CharIsIntegral         extends Integral<Object>
//! interface Numeric$BigIntIsIntegral       extends Integral<BigInt>
//! interface Numeric$DoubleIsFractional     extends Fractional<Object>
//! interface Numeric$FloatIsFractional      extends Fractional<Object>
//! interface Numeric$BigDecimalIsFractional extends Numeric$BigDecimalIsConflicted, Fractional<BigDecimal>
//! ```
//!
//! どれが implicit として実際に選ばれるかは、実 scalac に
//! `implicitly[…].getClass.getName` を出力させて確かめた（`BigDecimal` には
//! `BigDecimalAsIfIntegral` も居るが implicit ではない）。
//!
//! # 曖昧性が増えない理由
//!
//! `Ordering[Int]` の implicit scope（SLS 7.2）は `Ordering` とその親、および
//! `Int` の companion で、`Numeric` の companion は**入らない**。だから
//! `IntIsIntegral` に `Integral[Int]`（`<: Ordering[Int]`）を与えても
//! `Ordering[Int]` の候補は `Ordering.Int` のままで、増えない。実 scalac も
//! `implicitly[Ordering[Int]]` に `Ordering$Int$` を返す。

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    let Some(ordering) = crate::classpath::find_by_jvm(st, "scala/math/Ordering") else {
        return;
    };
    let Some(numeric) = crate::classpath::find_by_jvm(st, "scala/math/Numeric") else {
        return;
    };
    add_parent(st, numeric, ordering);
    if !library_abi {
        // 私有ランタイム（`--no-scala-library`）には `scala/math/Integral` の
        // classfile も `Numeric$IntIsIntegral$` も無い。ここで型だけ生やすと
        // 「読み込めないクラスを参照するバイトコード」になるので触らない。
        // 未実装は診断のまま（`.agent-brief.md` の「スタブ禁止」）。
        return;
    }
    let integral = ensure_typeclass(st, "scala/math/Integral", "Integral");
    let fractional = ensure_typeclass(st, "scala/math/Fractional", "Fractional");
    add_parent(st, integral, numeric);
    add_parent(st, fractional, numeric);
    retype_numeric_instances(st, numeric, integral, fractional);
    add_ordering_option(st, ordering);
}

/// `object Ordering` の `implicit def Option[T](implicit ord: Ordering[T]):
/// Ordering[Option[T]]`。
///
/// jar: `Ordering$.Option:(Lscala/math/Ordering;)Lscala/math/Ordering;`
/// （`javap -p -s scala.math.Ordering$`）。`Ordering.TupleN`
/// （`prelude_ordtuple.rs`）と同じ形の穴で、これが無いと
/// `List(Some(2), None).sorted` が `no implicit` になる。
fn add_ordering_option(st: &mut SymbolTable, ordering: SymbolId) {
    let Some(module) = crate::classpath::find_by_jvm(st, "scala/math/Ordering$") else {
        return;
    };
    let mcls = st.module_class_of(module);
    if !st.lookup_member(mcls, "Option").is_empty() {
        return;
    }
    let option = st.option_sym;
    if option.is_none() {
        return;
    }
    let m = crate::prelude::prelude_method(
        st,
        mcls,
        "Option",
        vec![],
        Type::Any,
        crate::symbol::Intrinsic::None,
    );
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    let t = crate::prelude::type_param(st, m, "T");
    st.get_mut(m).tparams = vec![t];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ordering,
            args: vec![Type::TypeParam(t)],
        }]],
        ret: Box::new(Type::Class {
            sym: ordering,
            args: vec![Type::Class {
                sym: option,
                args: vec![Type::TypeParam(t)],
            }],
        }),
    };
    if !st.get(module).members.contains(&m) {
        st.get_mut(module).members.push(m);
    }
}

/// `child[T] extends parent[T]`。`child` の最初の型パラメータをそのまま
/// 渡す（`Numeric[T] <: Ordering[T]`）。すでに同じ親を持つなら何もしない。
fn add_parent(st: &mut SymbolTable, child: SymbolId, parent: SymbolId) {
    if st.get(child).kind != SymKind::Class {
        return;
    }
    if st
        .get(child)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == parent))
    {
        return;
    }
    let args = match st.get(child).tparams.first().copied() {
        Some(tp) if !st.get(parent).tparams.is_empty() => vec![Type::TypeParam(tp)],
        _ => Vec::new(),
    };
    st.get_mut(child)
        .parents
        .push(Type::Class { sym: parent, args });
}

/// `trait <name>[T]` を prelude に用意し、`add_scala_aliases` と同じように
/// 無修飾名でも引けるようにする。
///
/// 型パラメータ名を `T` にするのは pickle 側と合わせるため:
/// `pickle_supply` は `st.get(cls).tparams` の**名前**でスコープを作って
/// `quot(T, T): T` を写すので、名前が違うと写せない。実ライブラリの
/// `trait Integral[T]` / `trait Fractional[T]` も `T`。
fn ensure_typeclass(st: &mut SymbolTable, jvm: &str, name: &str) -> SymbolId {
    let id = match crate::classpath::find_by_jvm(st, jvm) {
        Some(id) => id,
        None => {
            let (pkg, simple) = jvm.rsplit_once('/').unwrap_or(("", jvm));
            let owner = crate::classpath::ensure_package(st, pkg);
            let id = st.alloc(
                simple,
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
    };
    if st.get(id).tparams.is_empty() {
        let t = st.alloc("T", id, SymKind::TypeParam, Flags::EMPTY, "");
        st.get_mut(t).ty = Type::TypeParam(t);
        st.get_mut(id).tparams = vec![t];
    }
    let already = st
        .lookup(name)
        .into_iter()
        .any(|s| s == id && st.get(s).kind == SymKind::Class);
    if !already {
        st.enter_in_current(name, id);
    }
    id
}

/// `object Numeric` の implicit インスタンスに、実 ABI どおりの
/// `Integral[…]` / `Fractional[…]` を付け直し、足りない分を足す。
fn retype_numeric_instances(
    st: &mut SymbolTable,
    numeric: SymbolId,
    integral: SymbolId,
    fractional: SymbolId,
) {
    let Some(num_mod) = st.companion_module(numeric) else {
        return;
    };
    let num_cls = st.module_class_of(num_mod);
    let big_int = crate::classpath::find_by_jvm(st, "scala/math/BigInt")
        .map(|sym| Type::Class { sym, args: vec![] });
    let big_dec = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal")
        .map(|sym| Type::Class { sym, args: vec![] });
    let table: Vec<(&str, &str, SymbolId, Option<Type>)> = vec![
        (
            "IntIsIntegral",
            "scala/math/Numeric$IntIsIntegral$",
            integral,
            Some(Type::Int),
        ),
        (
            "LongIsIntegral",
            "scala/math/Numeric$LongIsIntegral$",
            integral,
            Some(Type::Long),
        ),
        (
            "ByteIsIntegral",
            "scala/math/Numeric$ByteIsIntegral$",
            integral,
            Some(Type::Byte),
        ),
        (
            "ShortIsIntegral",
            "scala/math/Numeric$ShortIsIntegral$",
            integral,
            Some(Type::Short),
        ),
        (
            "CharIsIntegral",
            "scala/math/Numeric$CharIsIntegral$",
            integral,
            Some(Type::Char),
        ),
        (
            "BigIntIsIntegral",
            "scala/math/Numeric$BigIntIsIntegral$",
            integral,
            big_int,
        ),
        (
            "DoubleIsFractional",
            "scala/math/Numeric$DoubleIsFractional$",
            fractional,
            Some(Type::Double),
        ),
        (
            "FloatIsFractional",
            "scala/math/Numeric$FloatIsFractional$",
            fractional,
            Some(Type::Float),
        ),
        (
            "BigDecimalIsFractional",
            "scala/math/Numeric$BigDecimalIsFractional$",
            fractional,
            big_dec,
        ),
    ];
    for (name, jvm, tc, arg) in table {
        let Some(arg) = arg else { continue };
        set_instance(st, num_cls, name, jvm, tc, arg);
    }
    // `Numeric.IntIsIntegral` のような修飾名でも引けるように、モジュール側にも
    // 同じメンバを持たせる（`prelude_seq::add_numeric` と同じ扱い）。
    let known: Vec<SymbolId> = st.get(num_mod).members.clone();
    for m in st.get(num_cls).members.clone() {
        if !known.contains(&m) {
            st.get_mut(num_mod).members.push(m);
        }
    }
}

/// `implicit object <name> extends <tc>[<arg>]`（`<jvm>.MODULE$`）を
/// 作る、または既にあるものの型を書き換える。
fn set_instance(
    st: &mut SymbolTable,
    num_cls: SymbolId,
    name: &str,
    jvm: &str,
    tc: SymbolId,
    arg: Type,
) {
    let ty = Type::Class {
        sym: tc,
        args: vec![arg],
    };
    // `add_implicit_instance` は module シンボルの `ty` を `ModuleRef` から
    // `Numeric[Int]` に**上書き**するので、`module_class_of` はもう
    // module class に辿り着けない。名前で引く。
    let members = st.get(num_cls).members.clone();
    let cls_name = format!("{name}$");
    let cls = match members
        .iter()
        .copied()
        .find(|s| st.get(*s).kind == SymKind::ModuleClass && st.get(*s).name == cls_name)
    {
        Some(c) => c,
        None => {
            let c = st.alloc(
                cls_name,
                num_cls,
                SymKind::ModuleClass,
                Flags::MODULE.with(Flags::FINAL),
                jvm,
            );
            st.get_mut(c).ty = Type::ModuleRef(c);
            c
        }
    };
    let m = match members
        .iter()
        .copied()
        .find(|s| st.get(*s).kind == SymKind::Module && st.get(*s).name == name)
    {
        Some(m) => m,
        None => st.alloc(name, num_cls, SymKind::Module, Flags::MODULE, jvm),
    };
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).ty = ty.clone();
    st.get_mut(cls).parents = vec![ty];
}
