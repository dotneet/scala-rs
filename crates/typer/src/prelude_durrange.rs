//! `scala.collection.immutable.Range`'s companion object and
//! `scala.concurrent.duration`'s postfix unit syntax (agent/durrange).
//!
//! Both are `--scala-library` only: the private runtime emits neither
//! `scala/collection/immutable/Range$` nor anything under
//! `scala/concurrent/duration`, and `prelude.rs` already gates the `Range`
//! *class* itself on `library_abi` (without the jar, `1 until 10` is a
//! diagnostic, not a `Range`).

use crate::check::Typer;
use crate::prelude::{method, module};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_span::Span;

/// `object Range` — the companion `Range$`, missing entirely until now.
///
/// The prelude declared the *class* `scala.collection.immutable.Range` and
/// nothing else, so the identifier `Range` in term position resolved to the
/// class symbol. `Range(0, 5)` then looked `apply` up among the *class's* own
/// members and found `Range.apply(i: Int): Int` — the element accessor — which
/// is where the `no matching overload for (Int)Int` came from. `Range.inclusive`
/// happened to work only because member lookup falls back to the companion the
/// jar supplies.
///
/// `javap -p -s scala.collection.immutable.Range$` (2.13.16) declares exactly
/// six methods; there is no `BigInt` / `Long` / `BigDecimal` overload of
/// `apply` or `inclusive` on `Range$` at all. Those live in the *nested*
/// objects `Range.Int` / `Range.Long` / `Range.BigInt` / `Range.BigDecimal`
/// (`Range$Long$` &c., each with its own `apply`/`inclusive` returning a
/// `NumericRange`), which is a separate feature and stays out of this slice.
///
/// The two `apply` overloads return `Range$Exclusive` and the two `inclusive`
/// ones `Range$Inclusive`, not the abstract `Range`, so each carries an
/// explicit JVM descriptor: a call emitted as `(II)Lscala/…/Range;` does not
/// link. `RichInt.to` already needed the same treatment in `gen.rs`.
pub fn install_range_companion(st: &mut SymbolTable) {
    let Some(range) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Range") else {
        return;
    };
    if st
        .lookup_member(st.scala_pkg, "Range")
        .into_iter()
        .any(|id| matches!(st.get(id).kind, SymKind::Module | SymKind::ModuleClass))
    {
        return;
    }
    let range_t = Type::Class {
        sym: range,
        args: vec![],
    };
    let m = module(
        st,
        st.scala_pkg,
        "Range",
        "scala/collection/immutable/Range$",
    );
    let mc = st.module_class_of(m);
    for (name, arity, ret_jvm) in [
        ("apply", 2, "Range$Exclusive"),
        ("apply", 3, "Range$Exclusive"),
        ("inclusive", 2, "Range$Inclusive"),
        ("inclusive", 3, "Range$Inclusive"),
    ] {
        let id = method(
            st,
            mc,
            name,
            vec![Type::Int; arity],
            range_t.clone(),
            Intrinsic::None,
        );
        st.set_jvm_name(
            id,
            format!(
                "({})Lscala/collection/immutable/{ret_jvm};",
                "I".repeat(arity)
            ),
        );
    }
    // `count(start, end, step)` / `count(start, end, step, isInclusive)`.
    let c3 = method(
        st,
        mc,
        "count",
        vec![Type::Int; 3],
        Type::Int,
        Intrinsic::None,
    );
    st.set_jvm_name(c3, "(III)I");
    let c4 = method(
        st,
        mc,
        "count",
        vec![Type::Int, Type::Int, Type::Int, Type::Boolean],
        Type::Int,
        Intrinsic::None,
    );
    st.set_jvm_name(c4, "(IIIZ)I");
    // A module's members are looked up on the module *symbol* as well as on
    // its module class, exactly as `add_ordering` mirrors `Ordering$`.
    let mems = st.get(mc).members.clone();
    st.get_mut(m).members.extend(mems);
    // `import_members(scala_pkg)` has already run by the time the prelude gets
    // here, so the new module has to be entered by hand (`Ordered` next to it
    // is entered the same way).
    st.enter_in_current("Range", m);
    // So has `add_package_paths`, which is what makes the *class* reachable as
    // `scala.collection.immutable.Range`. Register the companion there too, so
    // the qualified spelling picks it up in term position.
    let pkg = crate::classpath::ensure_package(st, "scala/collection/immutable");
    if !st.get(pkg).members.contains(&m) {
        st.get_mut(pkg).members.push(m);
    }
}

/// `object Ordered` and its one member,
/// `implicit def orderingToOrdered[T](x: T)(implicit ord: Ordering[T]): Ordered[T]`.
///
/// `javap -p -s scala.math.Ordered$` (2.13.16) shows exactly this one method,
/// erased to `(Ljava/lang/Object;Lscala/math/Ordering;)Lscala/math/Ordered;`.
/// It is how nsc satisfies an `A => Ordered[A]` view for a type that is not
/// itself `Ordered`: `h("a", "b")` eta-expands it to
/// `x => Ordered.orderingToOrdered(x)(Ordering.String)`, which is exactly the
/// `$anonfun$main$2` real scalac emits for
/// `def h[A](x: A, y: A)(implicit ev: A => Ordered[A])`.
///
/// `--scala-library` only: the private runtime writes `scala/math/Ordered`
/// (see `runtime.rs`) but neither `Ordered$` nor `Ordering`, so without the
/// jar the view has no witness and the search keeps reporting
/// `no implicit: could not find implicit value of type (Int) => Ordered[Int]`.
pub fn install_ordered_companion(st: &mut SymbolTable) {
    let Some(ordered) = crate::classpath::find_by_jvm(st, "scala/math/Ordered") else {
        return;
    };
    let Some(ordering) = crate::classpath::find_by_jvm(st, "scala/math/Ordering") else {
        return;
    };
    let math = crate::classpath::ensure_package(st, "scala/math");
    if st
        .lookup_member(math, "Ordered")
        .into_iter()
        .any(|id| matches!(st.get(id).kind, SymKind::Module | SymKind::ModuleClass))
    {
        return;
    }
    let m = module(st, math, "Ordered", "scala/math/Ordered$");
    let mc = st.module_class_of(m);
    let conv = st.alloc(
        "orderingToOrdered",
        mc,
        SymKind::Method,
        Flags::IMPLICIT,
        "",
    );
    let t = crate::prelude::type_param(st, conv, "T");
    let tt = Type::TypeParam(t);
    let ord_t = Type::Class {
        sym: ordering,
        args: vec![tt.clone()],
    };
    let ret = Type::Class {
        sym: ordered,
        args: vec![tt.clone()],
    };
    let x = st.alloc("x", conv, SymKind::Term, Flags::PARAM, "");
    st.get_mut(x).ty = tt.clone();
    let ev = st.alloc(
        "ord",
        conv,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = ord_t.clone();
    st.get_mut(conv).tparams = vec![t];
    st.get_mut(conv).params = vec![x, ev];
    st.get_mut(conv).paramss = vec![vec![x], vec![ev]];
    st.get_mut(conv).ty = Type::Method {
        paramss: vec![vec![tt], vec![ord_t]],
        ret: Box::new(ret),
    };
    // `T` erases to `Object` and `Ordering[T]` / `Ordered[T]` to their raw
    // classes; spell the descriptor out rather than rely on that.
    st.get_mut(conv).jvm_name =
        "(Ljava/lang/Object;Lscala/math/Ordering;)Lscala/math/Ordered;".into();
    let mems = st.get(mc).members.clone();
    st.get_mut(m).members.extend(mems);
}

/// The boxed representations behind `5.seconds`, paired with the primitive
/// each of them wraps.
const DURATION_BOXES: [(&str, &str, Type); 3] = [
    (
        "DurationInt",
        "scala/concurrent/duration/package$DurationInt",
        Type::Int,
    ),
    (
        "DurationLong",
        "scala/concurrent/duration/package$DurationLong",
        Type::Long,
    ),
    (
        "DurationDouble",
        "scala/concurrent/duration/package$DurationDouble",
        Type::Double,
    ),
];

impl Typer {
    /// `implicit def DurationInt(n: Int): DurationInt` and its `DurationLong` /
    /// `DurationDouble` siblings, installed on `scala.concurrent.duration`'s
    /// package object the first time that object is needed.
    ///
    /// Lazy rather than part of `install_prelude` because the unit methods
    /// return `scala.concurrent.duration.FiniteDuration`, and that symbol does
    /// not exist until its classfile is read out of the jar. Declaring an empty
    /// `FiniteDuration` up front to point at would be a stub: nothing loads a
    /// non-`java/` classfile onto a symbol the prelude already created
    /// (`ensure_java_loaded` bails out for it), so `fd.toMillis` would stop
    /// resolving.
    ///
    /// `javap -p -s scala.concurrent.duration.package$` shows the three
    /// conversions as `DurationInt(int)int`, `DurationLong(long)long` and
    /// `DurationDouble(double)double` — value classes, erased to the primitive
    /// they wrap. The classfile reader therefore read `DurationInt` as a method
    /// from `Int` to `Int`, and a classfile carries no `IMPLICIT` flag for it
    /// either (`PickleSupply::supply_implicit_members`, which could have read
    /// one out of the `ScalaSignature`, skips everything under `scala/`). That
    /// is the whole of why `5.seconds` reported
    /// `value seconds is not a member of 5`.
    ///
    /// The *boxed* `package$DurationInt` is a real class in the jar carrying
    /// all twenty unit methods — `nanoseconds` / `nanos` / `nanosecond` /
    /// `nano`, the same four for `micro` and `milli`, then `seconds` /
    /// `second`, `minutes` / `minute`, `hours` / `hour`, `days` / `day` — as
    /// ordinary instance methods returning `FiniteDuration`. They are *not*
    /// `$extension` statics: `javap` of `package$DurationInt$` shows only
    /// `durationIn$extension`, `hashCode$extension` and `equals$extension`
    /// there. So the class is loaded rather than re-declared, and the twenty
    /// (times three) unit methods come along with it; all this adds is the
    /// conversion.
    pub(crate) fn install_duration_syntax(&mut self, pkg: SymbolId, span: Span) {
        if self.st.get(pkg).jvm_name != "scala/concurrent/duration" {
            return;
        }
        let Some(pkg_mcls) = self
            .st
            .lookup_member(pkg, "package")
            .into_iter()
            .find(|&s| matches!(self.st.get(s).kind, SymKind::Module | SymKind::ModuleClass))
            .map(|m| self.st.module_class_of(m))
        else {
            return;
        };
        // Idempotent: `package_object_of` runs for every
        // `import scala.concurrent.duration._` in the run.
        if self
            .st
            .lookup_member(pkg_mcls, "DurationInt")
            .into_iter()
            .any(|m| self.st.get(m).flags.contains(Flags::IMPLICIT))
        {
            return;
        }
        for (name, jvm, under) in DURATION_BOXES {
            if !self.load_binary_into(jvm, pkg, span, false) {
                continue;
            }
            let Some(box_cls) = crate::classpath::find_by_jvm(&self.st, jvm) else {
                continue;
            };
            // Drop what the classfile reader left behind: `DurationInt(Int)Int`
            // and the real conversion share a name and an arity, so keeping
            // both would make every mention of the name ambiguous.
            let stale: Vec<SymbolId> = self
                .st
                .get(pkg_mcls)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    let s = self.st.get(m);
                    s.kind == SymKind::Method && s.name == name
                })
                .collect();
            self.st
                .get_mut(pkg_mcls)
                .members
                .retain(|m| !stale.contains(m));
            self.st.get_mut(pkg).members.retain(|m| !stale.contains(m));
            let id = method(
                &mut self.st,
                pkg_mcls,
                name,
                vec![under],
                Type::Class {
                    sym: box_cls,
                    args: vec![],
                },
                Intrinsic::NewWrapper,
            );
            self.st.get_mut(id).flags = self.st.get(id).flags.with(Flags::IMPLICIT);
        }
        // A package object's members are the package's members
        // (`package_object_of` makes the same copy for what the classfile gave).
        for mem in self.st.get(pkg_mcls).members.clone() {
            if !self.st.get(pkg).members.contains(&mem) {
                self.st.get_mut(pkg).members.push(mem);
            }
        }
    }
}
