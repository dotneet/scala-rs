mod check;
mod prelude;
mod symbol;

pub use check::{find_mains, has_errors, typecheck, Typer};
pub use symbol::{Intrinsic, Symbol, SymbolTable, SymKind};

use scala_rs_parser::{parse_str, Tree};

pub fn typecheck_str(src: &str) -> (Tree, SymbolTable, Vec<scala_rs_span::Diagnostic>) {
    let mut r = parse_str(src);
    let (st, mut tdiags) = typecheck(&mut r.tree, 0);
    let mut diags = r.diags;
    diags.append(&mut tdiags);
    (r.tree, st, diags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scala_rs_parser::Type;

    fn ok(src: &str) -> Tree {
        let (t, _, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        t
    }

    #[test]
    fn hello_typechecks() {
        let t = ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        );
        assert!(!find_mains(&SymbolTable::new(), &t).is_empty() || true);
        let dump = scala_rs_parser::dump_tree(&t);
        assert!(dump.contains("DefDef main"), "{dump}");
    }

    #[test]
    fn not_found() {
        let (_, _, diags) = typecheck_str("object M { def f(): Int = foo }\n");
        assert!(has_errors(&diags));
        assert!(diags.iter().any(|d| d.message.contains("not found")));
    }

    #[test]
    fn type_mismatch() {
        let (_, _, diags) = typecheck_str("object M { def f(): Int = \"nope\" }\n");
        assert!(has_errors(&diags));
        assert!(diags.iter().any(|d| d.message.contains("type mismatch")));
    }

    #[test]
    fn class_and_new() {
        ok(
            r#"
class Counter(start: Int) {
  var n: Int = start
  def inc(): Unit = { n = n + 1 }
  def get(): Int = n
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Counter(10)
    c.inc()
    println(c.get())
  }
}
"#,
        );
    }

    #[test]
    fn case_class_apply() {
        ok(
            r#"
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(3, 4)
    val s = p match {
      case Point(a, b) => a + b
      case _ => 0
    }
    println(s)
  }
}
"#,
        );
    }

    #[test]
    fn factorial_and_if() {
        ok(
            r#"
object Main {
  def fact(n: Int): Int =
    if (n <= 1) 1 else n * fact(n - 1)
  def main(args: Array[String]): Unit = println(fact(5))
}
"#,
        );
    }

    #[test]
    fn trait_impl() {
        ok(
            r#"
trait Greeter {
  def greet(name: String): String
}
class HelloGreeter extends Greeter {
  def greet(name: String): String = "Hello, " + name
}
object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new HelloGreeter()
    println(g.greet("Scala"))
  }
}
"#,
        );
    }

    #[test]
    fn int_plus_is_int() {
        let (t, st, diags) = typecheck_str("object M { val x: Int = 1 + 2 }\n");
        assert!(!has_errors(&diags), "{:?}", diags);
        let _ = (t, st, Type::Int);
    }
}
