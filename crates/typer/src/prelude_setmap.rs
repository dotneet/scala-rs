//! `Array` を `Seq` として渡すための `Predef` の包み込みメソッドと、
//! `scala.collection.Map` の読み出しメンバ（agent/setmap）。
//!
//! # `Array` → コレクション
//!
//! nsc の `-Xprint:typer` で確認した実際の挙動（2.13.16）:
//!
//! ```text
//! def v(a: Array[Any]): Seq[Any]          = scala.Predef.copyArrayToImmutableIndexedSeq[Any](a)
//! def w(a: Array[Int]): Seq[Int]          = scala.Predef.copyArrayToImmutableIndexedSeq[Int](a)
//! def y(a: Array[Any]): Iterable[Any]     = scala.Predef.genericWrapArray[Any](a)
//! ```
//!
//! `scala.Seq` / `scala.IndexedSeq` は **`immutable`** の別名なので、
//! `genericWrapArray` が返す `scala.collection.mutable.ArraySeq` では届かない。
//! そこで最下位（`LowPriorityImplicits2`）の `copyArrayToImmutableIndexedSeq`
//! が選ばれる。`scala.Iterable` は `scala.collection.Iterable` なので
//! `genericWrapArray` で届き、優先順位どおりそちらが選ばれる。
//!
//! ブリーフの仮説（「`genericWrapArray` は記述子が合わず使えない、
//! `wrapRefArray` を足せ」）は**誤り**だった。合わなかったのは
//! `Array[Any]` と書いたときの `([Ljava/lang/Object;)` で、
//! 本物の型パラメータを持たせて `Array[T]` と宣言すれば
//! `erasure.rs` の `array_elem_is_abstract` が nsc と同じく
//! `Ljava/lang/Object;` に潰す。javap:
//!
//! ```text
//! scala.LowPriorityImplicits:
//!   public <T> scala.collection.mutable.ArraySeq<T> genericWrapArray(java.lang.Object);
//! scala.LowPriorityImplicits2:
//!   public <T> scala.collection.immutable.IndexedSeq<T> copyArrayToImmutableIndexedSeq(java.lang.Object);
//! ```
//!
//! `wrapRefArray` は `T <: AnyRef` の制約があり `Array[Any]` には効かない
//! （nsc も上のとおり選んでいない）ので足さない。
//!
//! `wrapBooleanArray`（`prelude_seqfn.rs`）と同じ理由で **`implicit` にしない**:
//! implicit にすると普通の `Array` のメンバ選択で `refArrayOps` と競合する。
//! `seqfn_view.rs` が名前で引く。
//!
//! # `scala.collection.Map`
//!
//! `prelude_hier.rs` の `LINKS` が作る `scala/collection/Map` は
//! 型パラメータだけのつなぎで、メンバを 1 つも持たない。`scala/` 名の
//! prelude クラスは `pickle_supply::adopt_binary_class` が触らない
//! （`class_sym.0 < st.prelude_end`）ので、jar からも補われない。
//! slick の `expansions: collection.Map[TableIdentitySymbol, (TermSymbol, Node)]`
//! に対する `expansions contains tsym` が `not a member`、`expansions(tsym)` は
//! コンパニオンの可変長 `apply` に落ちて `no matching overload` になっていた。
//! `collection.MapOps` の読み出し側 3 つだけをここで宣言する。

use crate::prelude::{method, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// すべて `library_abi` 専用。私有ランタイム（`--no-scala-library`）には
/// `scala/collection/Map` も `IterableOps.++` も `Predef.genericWrapArray` も
/// 実体が無く、型だけ通して codegen で存在しないメソッドを呼ぶくらいなら、
/// 従来どおり診断を出す方が正しい（`.agent-brief.md` の「スタブ禁止」）。
pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    add_collection_map_members(st);
    add_option_is_iterable_once(st);
    add_set_widening_concat(st);
    add_array_wraps(st);
}

/// `Set.++[B >: A](that: IterableOnce[B]): Set[B]`。
///
/// 2.13 の `++` は **2 つのオーバーロード**である（`javap`）:
///
/// ```text
/// scala.collection.SetOps:      public default C   $plus$plus(scala.collection.IterableOnce<A>);
/// scala.collection.IterableOps: public default <B> CC $plus$plus(scala.collection.IterableOnce<B>);
/// ```
///
/// prelude 側には前者に相当する 1 つ（`prelude_coll` が `++(Set[A]): Set[A]`
/// として作り、`prelude_buildfrom::widen_set_concat` が
/// `++(IterableOnce[A]): Set[A]` に広げたもの）しか無く、しかもそれが
/// `lookup_member` に見つかるので pickle 側は `++` を一度も訊かれない
/// （`SCALA_RS_PICKLE_DEBUG=1` で確認: `concat` は訊かれるが `++` は訊かれない）。
/// そのため `s ++ anOptionOfSomethingElse` — slick の
/// `Set() ++ dbType.map(…) ++ (if(…) Some(…) else None) ++ …` — が
/// `no matching overload` になっていた。
///
/// 単相の方が適用できる限りそちらが厳密により specific なので、
/// 2 つ並べても nsc と同じ選び方になる。codegen は owner+名前で引く
/// （`gen.rs` の `is_stdlib_set` の `"++"`）ので、両方とも
/// `IterableOps.++` を呼ぶ 1 本の分岐で足りる。
fn add_set_widening_concat(st: &mut SymbolTable) {
    let (Some(set), Some(ioc)) = (
        crate::classpath::find_by_jvm(st, "scala/collection/immutable/Set"),
        crate::classpath::find_by_jvm(st, "scala/collection/IterableOnce"),
    ) else {
        return;
    };
    let Some(&a) = st.get(set).tparams.first() else {
        return;
    };
    let poly_already = st
        .get(set)
        .members
        .iter()
        .copied()
        .any(|m| st.get(m).name == "++" && !st.get(m).tparams.is_empty());
    if poly_already {
        return;
    }
    let m = st.alloc("++", set, SymKind::Method, Flags::EMPTY, "");
    let b = type_param(st, m, "B");
    st.get_mut(b).bound_lo = Some(Type::TypeParam(a));
    let tb = Type::TypeParam(b);
    let param_ty = Type::Class {
        sym: ioc,
        args: vec![tb.clone()],
    };
    let p = st.alloc("that", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p).ty = param_ty.clone();
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![p];
    st.get_mut(m).paramss = vec![vec![p]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![param_ty]],
        ret: Box::new(Type::Class {
            sym: set,
            args: vec![tb],
        }),
    };
    st.get_mut(m).intrinsic = Intrinsic::None;
}

/// `Option[A] <: IterableOnce[A]`。
///
/// 2.13 で `Option` は `IterableOnce` になった（2.12 の
/// `option2Iterable` 暗黙変換ではなく、本当の親）:
///
/// ```text
/// sealed abstract class Option[+A] extends IterableOnce[A] with Product with Serializable
/// ```
///
/// これが無いと slick の
/// `Set() ++ dbType.map(...) ++ (if(...) Some(...) else None) ++ …`
/// が `no matching overload for (IterableOnce[A])Set[A] with arguments
/// (Option[SqlType])` になる。実 scalac の `-Xprint:typer` は
/// `Set.apply[String]().++(o)` と、**変換なしで**そのまま渡している。
///
/// 辺だけを足す。`IterableOnce` はこの prelude では `foreach` しか持たず、
/// `Option` は自前の `foreach` を持つので、継承で増えるメンバは無い。
fn add_option_is_iterable_once(st: &mut SymbolTable) {
    let Some(ioc) = crate::classpath::find_by_jvm(st, "scala/collection/IterableOnce") else {
        return;
    };
    let opt = st.option_sym;
    if st.get(ioc).tparams.len() != 1 || st.get(opt).tparams.len() != 1 {
        return;
    }
    if st
        .get(opt)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == ioc))
    {
        return;
    }
    let a = Type::TypeParam(st.get(opt).tparams[0]);
    st.get_mut(opt).parents.push(Type::Class {
        sym: ioc,
        args: vec![a],
    });
}

/// `Predef.genericWrapArray[T](xs: Array[T]): mutable.ArraySeq[T]` と
/// `Predef.copyArrayToImmutableIndexedSeq[T](xs: Array[T]): immutable.IndexedSeq[T]`。
fn add_array_wraps(st: &mut SymbolTable) {
    let predef = st.predef;
    let owner = match st.get(predef).ty.clone() {
        Type::ModuleRef(id) => id,
        _ => predef,
    };
    if let Some(arrseq) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/ArraySeq") {
        add_wrap(st, owner, "genericWrapArray", arrseq);
    }
    if let Some(ixs) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/IndexedSeq") {
        add_wrap(st, owner, "copyArrayToImmutableIndexedSeq", ixs);
    }
}

/// 1 つの型パラメータ `T` を持つ `Array[T] => Cls[T]` を `owner` に足す。
///
/// `Array[T]`（`T` は抽象）の erasure は `Ljava/lang/Object;` なので、
/// 実 ABI の記述子とそのまま一致する。返りは `Cls[T]` で
/// `Lscala/collection/...;`。
fn add_wrap(st: &mut SymbolTable, owner: SymbolId, name: &str, cls: SymbolId) {
    if !st.lookup(name).is_empty() {
        return;
    }
    let m = st.alloc(name, owner, SymKind::Method, Flags::EMPTY, "");
    let t = type_param(st, m, "T");
    let tt = Type::TypeParam(t);
    let param = st.alloc("xs", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(param).ty = Type::Array(Box::new(tt.clone()));
    st.get_mut(m).tparams = vec![t];
    st.get_mut(m).params = vec![param];
    st.get_mut(m).paramss = vec![vec![param]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Array(Box::new(tt.clone()))]],
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![tt],
        }),
    };
    st.get_mut(m).intrinsic = Intrinsic::None;
    st.get_mut(owner).members.push(m);
    st.enter_in_current(name, m);
}

/// `collection.MapOps` の読み出しメンバ 3 つ。
///
/// javap（`scala.collection.MapOps`）:
/// ```text
/// public abstract scala.Option<V> get(K);
/// public V apply(K);
/// public boolean contains(K);
/// ```
fn add_collection_map_members(st: &mut SymbolTable) {
    let Some(map) = crate::classpath::find_by_jvm(st, "scala/collection/Map") else {
        return;
    };
    if st.get(map).tparams.len() != 2 {
        return;
    }
    let k = Type::TypeParam(st.get(map).tparams[0]);
    let v = Type::TypeParam(st.get(map).tparams[1]);
    if st
        .get(map)
        .members
        .iter()
        .any(|&m| st.get(m).name == "contains")
    {
        return;
    }
    method(
        st,
        map,
        "contains",
        vec![k.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "apply",
        vec![k.clone()],
        v.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "get",
        vec![k],
        Type::Class {
            sym: st.option_sym,
            args: vec![v],
        },
        Intrinsic::None,
    );
}
