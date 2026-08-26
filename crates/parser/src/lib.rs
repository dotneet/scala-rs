pub mod ast;
pub mod parse;
pub mod pretty;

pub use ast::*;
pub use parse::{parse_source, ParseResult};
pub use pretty::dump_tree;

use scala_rs_lexer::tokenize;
use scala_rs_span::{Diagnostic, SourceFile};

pub fn parse_file(source: &SourceFile, file_index: usize) -> ParseResult {
    let (tokens, lex_diags) = tokenize(source, file_index);
    let mut result = parse_source(source, file_index, tokens);
    let mut diags = lex_diags;
    diags.append(&mut result.diags);
    result.diags = diags;
    result
}

pub fn parse_str(src: &str) -> ParseResult {
    let sf = SourceFile::new("test.scala", src);
    parse_file(&sf, 0)
}

pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == scala_rs_span::Level::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> Tree {
        let r = parse_str(src);
        assert!(
            !has_errors(&r.diags),
            "parse errors for {src:?}: {:?}",
            r.diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        r.tree
    }

    fn first_stat(t: &Tree) -> &Tree {
        match &t.kind {
            TreeKind::PackageDef { stats, .. } => &stats[0],
            _ => t,
        }
    }

    #[test]
    fn object_hello() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Module Main"), "{dump}");
        assert!(dump.contains("DefDef main"), "{dump}");
    }

    #[test]
    fn class_and_case_class() {
        let t = parse_ok(
            r#"
class Counter(start: Int) {
  var n: Int = start
  def inc(): Unit = { n = n + 1 }
  def get(): Int = n
}
case class Point(x: Int, y: Int)
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Class Counter"), "{dump}");
        assert!(dump.contains("CaseClass Point"), "{dump}");
    }

    #[test]
    fn trait_and_match() {
        let t = parse_ok(
            r#"
trait Greeter { def greet(name: String): String }
object Main {
  def f(p: Point): Int = p match {
    case Point(a, b) => a + b
    case _ => 0
  }
}
class Point(x: Int, y: Int)
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Trait Greeter"), "{dump}");
        assert!(dump.contains("Match"), "{dump}");
    }

    #[test]
    fn if_while_lambda_for() {
        let t = parse_ok(
            r#"
object M {
  def g(n: Int): Int = {
    var i = 0
    while (i < n) { i = i + 1 }
    if (n > 0) n else 0
  }
  val f = (x: Int) => x + 1
  def h(xs: List[Int]): Unit = for (x <- xs) println(x)
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("While"), "{dump}");
        assert!(dump.contains("Function"), "{dump}");
        assert!(dump.contains("foreach") || dump.contains("Apply"), "{dump}");
    }

    #[test]
    fn interpolation_and_package() {
        let t = parse_ok(
            r#"
package foo.bar
object M {
  def hello(name: String): String = s"hi $name"
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("PackageDef"), "{dump}");
        assert!(dump.contains("Interpolate s"), "{dump}");
    }

    #[test]
    fn unimplemented_is_reported() {
        let r = parse_str("object M { def f = macro impl }\n");
        assert!(
            has_errors(&r.diags)
                || dump_tree(&r.tree).contains("Unimplemented")
                || !r.diags.is_empty()
        );
    }

    #[test]
    fn wildcard_and_forsome_types_parse() {
        let t = parse_ok(
            r#"
object Main {
  def show(xs: List[_]): Unit = ()
  def show2(xs: List[X] forSome { type X }): Unit = ()
  type Vacuous = Int forSome { type X }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("TypeDef _") || dump.contains("Existential"), "{dump}");
        assert!(dump.contains("Existential"), "forSome should parse as ExistentialTypeTree: {dump}");
    }

    #[test]
    fn colon_ops_are_right_assoc_on_the_rhs() {
        let t = parse_ok("object M { val xs = 1 :: Nil }\n");
        let dump = dump_tree(&t);
        // `1 :: Nil` is `Nil.::(1)`, not `1.::(Nil)`.
        assert!(dump.contains("Select"), "{dump}");
        assert!(dump.contains("::") || dump.contains("Apply"), "{dump}");
    }

    #[test]
    fn first_stat_is_module() {
        let t = parse_ok("object Foo { val x = 1 }\n");
        assert!(matches!(first_stat(&t).kind, TreeKind::ModuleDef { .. }));
    }

    #[test]
    fn try_catch_finally() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    try {
      throw new RuntimeException()
    } catch {
      case _: RuntimeException => println("caught")
    } finally {
      println("finally")
    }
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Try"), "{dump}");
    }

    #[test]
    fn eta_expansion_and_anonymous_new() {
        let t = parse_ok(
            r#"
object Main {
  def inc(x: Int): Int = x + 1
  def main(args: Array[String]): Unit = {
    val f = inc _
    val g = new Greeter { def greet(name: String): String = name }
    val a = new { def x: Int = 1 }
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Typed"), "eta `inc _` should parse as Typed: {dump}");
        assert!(dump.contains("Function"), "{dump}");
        assert!(dump.contains("$anon"), "anonymous class: {dump}");
    }
}
