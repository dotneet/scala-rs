//! The typer shape consumed by the backend's `Aux` pickle special case.

use scala_rs_parser::{RefineDecl, Type};
use scala_rs_typer::{has_errors, typecheck_str, SymKind};

#[test]
fn aux_keeps_captured_alias_and_refinement_parameter_separate() {
    let src = r#"
trait P[M[_]] { type F[_] }
object P { type Aux[M[_], F0[_]] = P[M] { type F[x] = F0[x] } }
"#;
    let (_tree, st, diags) = typecheck_str(src);
    assert!(!has_errors(&diags), "unexpected diagnostics: {diags:?}");
    let aux = st
        .symbols
        .iter()
        .find(|s| s.name == "Aux" && s.kind == SymKind::TypeMember && s.is_type_alias)
        .expect("Aux type alias");
    let Type::Refined { parents, decls } = &aux.ty else {
        panic!("Aux should retain its refinement, got {:?}", aux.ty);
    };
    assert_eq!(parents.len(), 1);
    let RefineDecl::Type {
        name,
        rhs: Some(Type::Applied { ctor, args }),
        tparams,
        ..
    } = decls.first().expect("Aux refinement member")
    else {
        panic!("Aux should contain an applied higher-kinded type member");
    };
    assert_eq!(name, "F");
    assert_eq!(*tparams, 1);
    assert_eq!(args.len(), 1);
    assert!(matches!(ctor.as_ref(), Type::TypeMember(_)));
}
