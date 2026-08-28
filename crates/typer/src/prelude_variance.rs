//! Declaration-site variance for the prelude's own class symbols.
//!
//! The prelude builds `List`, `Option`, `Seq` and friends by hand, and until
//! now every one of their type parameters was invariant. That silently broke
//! variance checking of *user* classes: `class C[+A](val xs: List[A])` was
//! rejected because `A` looked like it stood in an invariant position.
//!
//! The variances here are the ones `scala.collection` and `scala.util`
//! actually declare in 2.13; a class not listed keeps all parameters
//! invariant, which is the right default for the mutable collections.

use crate::symbol::SymbolTable;
use scala_rs_parser::ast::Flags;

/// `(jvm name, one character per type parameter)`, `+`/`-`/`=`.
const VARIANCES: &[(&str, &str)] = &[
    ("scala/collection/immutable/$colon$colon", "+"),
    ("scala/collection/immutable/IndexedSeq", "+"),
    ("scala/collection/immutable/LazyList", "+"),
    ("scala/collection/immutable/List", "+"),
    ("scala/collection/immutable/Map", "=+"),
    ("scala/collection/immutable/Queue", "+"),
    ("scala/collection/immutable/Seq", "+"),
    ("scala/collection/immutable/SortedMap", "=+"),
    ("scala/collection/immutable/TreeMap", "=+"),
    ("scala/collection/immutable/Vector", "+"),
    ("scala/collection/Iterable", "+"),
    ("scala/collection/IterableOnce", "+"),
    ("scala/collection/Iterator", "+"),
    ("scala/collection/MapView", "=+"),
    ("scala/collection/SeqView", "+"),
    ("scala/collection/View", "+"),
    ("scala/collection/WithFilter", "++"),
    ("scala/Option", "+"),
    ("scala/Option$WithFilter", "+"),
    ("scala/PartialFunction", "-+"),
    ("scala/Some", "+"),
    ("scala/util/Either", "++"),
    ("scala/util/Either$LeftProjection", "++"),
    ("scala/util/Failure", "+"),
    ("scala/util/Left", "++"),
    ("scala/util/Right", "++"),
    ("scala/util/Success", "+"),
    ("scala/util/Try", "+"),
    ("scala/util/Try$WithFilter", "+"),
];

pub fn install(st: &mut SymbolTable) {
    for i in 0..st.symbols.len() {
        let jvm = st.symbols[i].jvm_name.clone();
        let tps = st.symbols[i].tparams.clone();
        if tps.is_empty() {
            continue;
        }
        let spec = if let Some(n) = tuple_arity(&jvm) {
            "+".repeat(n)
        } else {
            match VARIANCES.iter().find(|(j, _)| *j == jvm) {
                Some((_, v)) => (*v).to_string(),
                None => continue,
            }
        };
        for (tp, c) in tps.iter().zip(spec.chars()) {
            let f = match c {
                '+' => Flags::COVARIANT,
                '-' => Flags::CONTRAVARIANT,
                _ => continue,
            };
            let cur = st.get(*tp).flags;
            st.get_mut(*tp).flags = cur.with(f);
        }
    }
}

/// `scala/TupleN` for `N` in 1..=22; every parameter is covariant.
fn tuple_arity(jvm: &str) -> Option<usize> {
    let n: usize = jvm.strip_prefix("scala/Tuple")?.parse().ok()?;
    (1..=22).contains(&n).then_some(n)
}
