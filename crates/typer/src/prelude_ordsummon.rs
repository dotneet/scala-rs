//! `scala` パッケージオブジェクトの `val Ordering = scala.math.Ordering` 相当。
//!
//! nsc の `package object scala`（`src/library/scala/package.scala`）は、
//! 型クラスを**型と項の両方**で無修飾に見えるようにしている:
//!
//! ```scala
//! type Ordering[T] = scala.math.Ordering[T]
//! val  Ordering    = scala.math.Ordering
//! ```
//!
//! `prelude::add_scala_aliases` は前者（`type`）だけを入れていた。
//! `st.enter_in_current("Ordering", <trait>)` で入るのは *class* シンボル
//! だけなので、**項**位置の `Ordering` もこの trait に解決される:
//!
//! - `Ordering.Int` は「trait `Ordering` のメンバ `Int`」を探しに行って
//!   `value Int is not a member of Ordering` になる。`scala.math.Ordering.Int`
//!   と完全修飾すれば通っていたのが証拠。
//!   （`agent/integral` が `Ordering.Option` という implicit を足したあとは、
//!   メンバが見つからない受け手に対する暗黙変換の探索がこの
//!   `Ordering[T] => Ordering[Option[T]]` を拾い、エラー文が
//!   `… is not a member of Ordering[Option[AnyRef]]` に化けていた。
//!   化けていたのはメッセージだけで、原因はここ。）
//! - `Ordering[String]` は「trait を項に置いて型適用した」形になり、型検査を
//!   **黙って通った**うえで codegen が `Ordering$.MODULE$` を積んで
//!   `Ordering` に checkcast する。実行時 `ClassCastException:
//!   scala.math.Ordering$ cannot be cast to scala.math.Ordering`。
//!
//! ここでコンパニオン module を同じスコープに入れると、`SymbolTable::lookup`
//! は class と module の両方を返し、項位置（`check::type_ident`）は
//! module を、型位置（`check::resolve_type_name`）は class を選ぶ。
//!
//! summon 側（`Ordering[String]` = `Ordering.apply[String]`）は
//! `check.rs` の module→`apply` リダイレクトが受け持つ。jar の
//! `Ordering$.apply:(Lscala/math/Ordering;)Lscala/math/Ordering;` は pickle から
//! 供給されるので、ここでシグネチャを手書きはしない（手書きすると
//! pickle 由来のものと二重に生えてオーバーロードになる）。
//!
//! `--no-scala-library` では `scala/math/Ordering` の classfile も
//! `Ordering$` も無く、`add_scala_aliases` 自体が何も入れないため
//! `not found: value Ordering` の診断のまま。ここも `library_abi` で塞ぐ。

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::symbol::{SymKind, SymbolTable};

/// `add_scala_aliases` が型として入れた別名と同じ綴りで、コンパニオン
/// module を項の名前空間にも入れる。
const ALIASES: [&str; 7] = [
    "scala/math/Ordering",
    "scala/math/Numeric",
    "scala/math/Equiv",
    "scala/math/Fractional",
    "scala/math/Integral",
    "scala/math/BigInt",
    "scala/math/BigDecimal",
];

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    for jvm in ALIASES {
        let Some(cls) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        if st.get(cls).kind != SymKind::Class {
            continue;
        }
        let name = st.get(cls).name.clone();
        let m = match st.companion_module(cls) {
            Some(m) => m,
            // `prelude_numhier::ensure_typeclass` は jar を読まずに
            // `trait Integral[T]` / `trait Fractional[T]` を生やすので、
            // コンパニオンが片方だけ無い状態になる。jar には
            // `scala/math/Integral$.apply:(Lscala/math/Integral;)Lscala/math/Integral;`
            // が実在する（`javap -p -s scala.math.Integral$`）ので、
            // ここで module を用意して初めて `Integral[Int]` が
            // `Integral.apply[Int]` になる。用意しないと trait そのものが
            // 項に立ち、`val i: Integral[Int] = Integral[Int]` が**黙って通って**
            // 実行時 `ClassCastException: scala.math.Integral$ cannot be cast to
            // scala.math.Integral` になっていた。
            None => make_companion(st, cls, jvm, &name),
        };
        if st.lookup(&name).into_iter().any(|s| s == m) {
            continue;
        }
        st.enter_in_current(&name, m);
    }
}

/// `object <name>`（`<jvm>$`）を class と同じ owner の下に作る。
fn make_companion(st: &mut SymbolTable, cls: SymbolId, jvm: &str, name: &str) -> SymbolId {
    let owner = st.get(cls).owner;
    let module_jvm = format!("{jvm}$");
    let mcls = st.alloc(
        format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        &module_jvm,
    );
    st.get_mut(mcls).ty = Type::ModuleRef(mcls);
    st.get_mut(mcls).parents = vec![Type::AnyRef];
    let m = st.alloc(name, owner, SymKind::Module, Flags::MODULE, &module_jvm);
    st.get_mut(m).ty = Type::ModuleRef(mcls);
    m
}
