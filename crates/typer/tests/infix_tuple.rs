use scala_rs_typer::{has_errors, typecheck_str};

#[test]
fn infix_tuple_arguments_are_expanded_to_method_parameters() {
    let src = r#"
        class C {
          def pair(a: Int, b: String): (Int, String) = (a, b)
        }
        object O {
          val value = new C pair (1, "one")
        }
    "#;
    let (_, _, diags) = typecheck_str(src);
    assert!(!has_errors(&diags), "unexpected diagnostics: {diags:?}");
}
