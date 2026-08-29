//! Links the prelude's `TupleN` classes to the `scala.Product` hierarchy.
//!
//! nsc declares every tuple as
//! `final case class TupleN[+T1, …](_1: T1, …) extends ProductN[T1, …] with Product with Serializable`,
//! but the prelude builds `Tuple2` (`prelude.rs`) and `Tuple3`…`Tuple22`
//! (`prelude_tuple.rs`) with `AnyRef` as their only parent. Without the real
//! edge a tuple is not a `Product`, so `def buildTuple(…): Product = … new
//! Tuple4(a, b, c, d)` — slick's generated `TupleSupport` — fails on every
//! arity, and so does any plain `val p: Product = (1, 2)`.
//!
//! `scala.Product` and `java.io.Serializable` belong to the classpath, not the
//! prelude, so this runs *after* the classpath is installed and links nothing
//! when those classes are absent: the private runtime (`--no-scala-library`)
//! ships a `scala/Tuple2` that implements neither, and claiming otherwise
//! would be a lie the backend could not back up.

use crate::symbol::SymbolTable;
use scala_rs_parser::Type;

/// Highest arity scala-library defines.
const MAX_TUPLE: usize = 22;

pub(crate) fn link_tuple_products(st: &mut SymbolTable) {
    let extra: Vec<Type> = ["scala/Product", "java/io/Serializable"]
        .iter()
        .filter_map(|jvm| crate::classpath::find_by_jvm(st, jvm))
        .map(|sym| Type::Class { sym, args: vec![] })
        .collect();
    if extra.is_empty() {
        return;
    }
    for n in 2..=MAX_TUPLE {
        let jvm = format!("scala/Tuple{n}");
        let Some(cls) = crate::classpath::find_by_jvm(st, &jvm) else {
            continue;
        };
        for p in &extra {
            if !st.get(cls).parents.contains(p) {
                st.get_mut(cls).parents.push(p.clone());
            }
        }
    }
}
