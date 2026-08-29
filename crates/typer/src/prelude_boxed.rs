//! `java.lang.Integer` and friends: the *boxes*, which are not `scala.Int`.
//!
//! nsc keeps two unrelated symbols here. `scala.Int` is a value class;
//! `java.lang.Integer` is an ordinary final Java class that happens to be the
//! *representation* `Int` erases to when it has to be boxed. `prelude.rs`
//! records that representation in `scala.Int`'s `jvm_name`, which is what
//! codegen needs (`1.toString` really is `Integer.toString`), but that field
//! is an erasure, not an identity — and `classpath::find_by_jvm` used to read
//! it as one. Installing the real `java.lang.Integer` classfile therefore
//! found `scala.Int`, poured `Integer`'s members into it and never entered
//! `Integer` into `java.lang`, so `java.lang.Integer.valueOf` reported "value
//! Integer is not a member of <notype>". `find_by_jvm` now skips the value
//! classes (see `SymbolTable::is_primitive_value_class`) and this module gives
//! the eight wrappers symbols of their own.
//!
//! With the two separated, converting between them needs the conversions nsc
//! puts in `Predef`. All sixteen exist in the real 2.13 `Predef`
//! (`int2Integer` / `Integer2int` / ...), so `library_abi` code could call
//! them; they are intrinsics instead, because `Predef.int2Integer` *is*
//! `Integer.valueOf` and `Predef.Integer2int` *is* `Integer.intValue`, and
//! emitting those directly works on the private runtime too, where there is no
//! `scala/Predef$.int2Integer`.

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// The eight primitive/wrapper pairs, with the `Predef` conversion names nsc
/// uses for each direction.
const BOXES: [(&str, &str, &str); 8] = [
    ("java/lang/Boolean", "boolean2Boolean", "Boolean2boolean"),
    ("java/lang/Byte", "byte2Byte", "Byte2byte"),
    ("java/lang/Short", "short2Short", "Short2short"),
    ("java/lang/Character", "char2Character", "Character2char"),
    ("java/lang/Integer", "int2Integer", "Integer2int"),
    ("java/lang/Long", "long2Long", "Long2long"),
    ("java/lang/Float", "float2Float", "Float2float"),
    ("java/lang/Double", "double2Double", "Double2double"),
];

fn primitive_of(jvm: &str) -> Type {
    match jvm {
        "java/lang/Boolean" => Type::Boolean,
        "java/lang/Byte" => Type::Byte,
        "java/lang/Short" => Type::Short,
        "java/lang/Character" => Type::Char,
        "java/lang/Integer" => Type::Int,
        "java/lang/Long" => Type::Long,
        "java/lang/Float" => Type::Float,
        _ => Type::Double,
    }
}

/// The JVM descriptor letter the box/unbox intrinsic carries, so codegen knows
/// which `valueOf` / `xxxValue` pair to emit without re-deriving it from the
/// (possibly narrower) static type of the argument.
fn desc_of(jvm: &str) -> &'static str {
    match jvm {
        "java/lang/Boolean" => "Z",
        "java/lang/Byte" => "B",
        "java/lang/Short" => "S",
        "java/lang/Character" => "C",
        "java/lang/Integer" => "I",
        "java/lang/Long" => "J",
        "java/lang/Float" => "F",
        _ => "D",
    }
}

/// Called at the end of `install_prelude`, i.e. *after* `import_members` has
/// opened `java.lang` into the root scope. The order matters: `java.lang.Byte`
/// and `scala.Byte` share a simple name, and in Scala the `scala._` import
/// wins, so the wrappers must not be swept into scope wholesale. The three
/// names that cannot collide with a `scala` type are entered by hand, which is
/// what `import java.lang._` gives in nsc.
pub(crate) fn install(st: &mut SymbolTable) {
    let predef_cls = st.module_class_of(st.predef);
    if predef_cls.is_none() {
        return;
    }
    for (jvm, to_box, from_box) in BOXES {
        let wrapper = crate::classpath::find_or_stub_java_class(st, jvm);
        let boxed = Type::Class {
            sym: wrapper,
            args: vec![],
        };
        let prim = primitive_of(jvm);
        let desc = desc_of(jvm);
        add_conversion(
            st,
            predef_cls,
            to_box,
            prim.clone(),
            boxed.clone(),
            Intrinsic::BoxValue(desc),
        );
        add_conversion(
            st,
            predef_cls,
            from_box,
            boxed,
            prim,
            Intrinsic::UnboxValue(desc),
        );
    }
    for (name, jvm) in [
        ("Integer", "java/lang/Integer"),
        ("Character", "java/lang/Character"),
        ("Number", "java/lang/Number"),
    ] {
        let id = crate::classpath::find_or_stub_java_class(st, jvm);
        st.enter_in_current(name, id);
    }
}

/// A `Predef` implicit conversion. `install_prelude` has already synced
/// `Predef`'s module-class members onto the module value and into the root
/// scope, so — exactly as `prelude_conform`'s `$conforms` does — each new
/// member has to do its own one-symbol sync rather than re-copying the list.
fn add_conversion(
    st: &mut SymbolTable,
    predef_cls: SymbolId,
    name: &str,
    from: Type,
    to: Type,
    intrinsic: Intrinsic,
) {
    if st
        .lookup_member(predef_cls, name)
        .iter()
        .any(|&m| st.get(m).kind == SymKind::Method)
    {
        return;
    }
    let id = st.alloc(
        name,
        predef_cls,
        SymKind::Method,
        Flags::FINAL.with(Flags::IMPLICIT),
        "",
    );
    // The parameter needs a *symbol*, not just a type: `only_implicit_clauses`
    // reads `paramss` off the symbol, and a conversion with no parameter
    // symbols looks like a nullary implicit *value*. Without this, `implicit y:
    // Int` resolved to `Integer2int` and reported a diverging expansion
    // instead of "no implicit found".
    let p = st.alloc("x", id, SymKind::Term, Flags::EMPTY, "");
    st.get_mut(p).ty = from.clone();
    st.get_mut(id).params = vec![p];
    st.get_mut(id).paramss = vec![vec![p]];
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![from]],
        ret: Box::new(to),
    };
    st.get_mut(id).intrinsic = intrinsic;
    st.get_mut(st.predef).members.push(id);
    st.enter_in_current(name, id);
}
