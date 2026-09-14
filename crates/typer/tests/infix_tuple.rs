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

#[test]
fn implicit_extension_intersection_receiver_keeps_concrete_type() {
    let src = r#"
        trait R[A]
        object Syntax {
          implicit class Ops[T <: R[_]](val value: T with R[_]) {
            def pair[U <: R[_]](other: U with R[_]): (T, U) = (value, other)
          }
        }
        object O {
          import Syntax._
          class C extends R[Int]
          val value: (C, C) = new C pair new C
        }
    "#;
    let (_, _, diags) = typecheck_str(src);
    assert!(!has_errors(&diags), "unexpected diagnostics: {diags:?}");
}
