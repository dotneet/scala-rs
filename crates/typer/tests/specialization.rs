//! Stage 1 of `@specialized`: the annotation is accepted, and what it selects
//! is recorded on the type parameter's symbol.
//!
//! Nothing downstream reads the record yet -- no `Foo$mcI$sp` class is emitted
//! -- and `tests/spec_classfiles.sh` is the ledger that keeps saying so. What
//! these tests pin down is that the record is right, so stage 2 starts from a
//! correct reading of the annotation rather than re-deriving one.

use scala_rs_parser::SpecializedTypes;
use scala_rs_typer::{typecheck_str, SymKind, SymbolTable};

fn compile(src: &str) -> SymbolTable {
    let (_, st, diags) = typecheck_str(src);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.level == scala_rs_span::Level::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(errors.is_empty(), "unexpected diagnostics: {errors:?}");
    st
}

/// The `@specialized` selection recorded for type parameter `owner[tparam]`.
fn recorded(st: &SymbolTable, owner: &str, tparam: &str) -> Option<SpecializedTypes> {
    let sym = st
        .symbols
        .iter()
        .find(|s| {
            s.kind == SymKind::TypeParam
                && s.name == tparam
                && st.get(s.owner).name == owner
                && !s.owner.is_none()
        })
        .unwrap_or_else(|| panic!("no type parameter {owner}[{tparam}]"));
    sym.specialized
}

fn names(st: &SymbolTable, owner: &str, tparam: &str) -> Vec<&'static str> {
    recorded(st, owner, tparam)
        .unwrap_or_else(|| panic!("{owner}[{tparam}] carries no @specialized"))
        .names()
}

#[test]
fn a_bare_annotation_selects_the_nine_primitives() {
    let st = compile("class C[@specialized T](val x: T)");
    assert_eq!(
        names(&st, "C", "T"),
        vec!["Byte", "Short", "Char", "Int", "Long", "Float", "Double", "Boolean", "Unit"]
    );
}

#[test]
fn an_argument_list_narrows_it() {
    let st = compile("class C[@specialized(Int, Long) T](val x: T)");
    assert_eq!(names(&st, "C", "T"), vec!["Int", "Long"]);
}

/// The argument is never resolved -- nsc reads the annotation in a phase after
/// the typer, and this reads it off the tree -- so `BestOfBreed` needs no
/// `import Specializable._` to be understood here, and both spellings work.
#[test]
fn a_group_expands_to_its_members() {
    let st = compile(
        "class C[@specialized(Specializable.Bits32AndUp) T](val x: T)\n\
         class D[@specialized(BestOfBreed) U](val x: U)",
    );
    assert_eq!(names(&st, "C", "T"), vec!["Int", "Long", "Float", "Double"]);
    assert_eq!(
        names(&st, "D", "U"),
        vec!["Int", "Double", "Boolean", "Unit", "AnyRef"]
    );
}

#[test]
fn a_method_type_parameter_records_it_too() {
    let st = compile("class C { def f[@specialized(Double) A](a: A): A = a }");
    assert_eq!(names(&st, "f", "A"), vec!["Double"]);
}

/// The fully qualified spelling is one name here, as it is in nsc. The
/// *renamed* spelling (`import scala.{specialized => sp}`) is resolved in the
/// parser, and is covered there and by the `sp_alias` e2e fixture -- the
/// import itself needs `scala.specialized` to exist, which it does only in
/// library mode.
#[test]
fn a_qualified_spelling_is_the_same_annotation() {
    let st = compile("class C[@scala.specialized(Boolean) T](val x: T)");
    assert_eq!(names(&st, "C", "T"), vec!["Boolean"]);
}

#[test]
fn an_unannotated_parameter_records_nothing() {
    let st = compile("class C[T](val x: T)");
    assert_eq!(recorded(&st, "C", "T"), None);
}

/// `@unspecialized` lands on the member, not on a type parameter.
#[test]
fn unspecialized_is_recorded_on_the_method() {
    let st = compile(
        "class C[@specialized T](val x: T) {\n\
         @scala.annotation.unspecialized def f(): T = x\n\
         def g(): T = x\n\
         }",
    );
    let method = |name: &str| {
        st.symbols
            .iter()
            .find(|s| s.kind == SymKind::Method && s.name == name)
            .unwrap_or_else(|| panic!("no method {name}"))
    };
    assert!(method("f").unspecialized);
    assert!(!method("g").unspecialized);
}

/// The annotation is a performance hint, and the typer has no rule that reads
/// it. A program that is wrong stays wrong under it.
#[test]
fn the_annotation_does_not_soften_type_checking() {
    let (_, _, diags) = typecheck_str("class C[@specialized T](val x: T) { val n: Int = x }");
    assert!(
        scala_rs_typer::has_errors(&diags),
        "expected a type error, got {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
