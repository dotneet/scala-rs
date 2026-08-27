pub mod ast;
pub mod finterp;
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
  def pad(n: Int): String = f"$n%02d"
  def rawnl: String = raw"a\nb"
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("PackageDef"), "{dump}");
        assert!(dump.contains("Interpolate s"), "{dump}");
        assert!(dump.contains("Interpolate f"), "{dump}");
        assert!(dump.contains("Interpolate raw"), "{dump}");
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
    fn constant_types_parse() {
        let t = parse_ok(
            r#"
object Main {
  val x: 1 = 1
  def f(n: 1): Int = n
  val b: true = true
  val s: "hi" = "hi"
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Literal"), "type-position literals: {dump}");
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
        assert!(
            dump.contains("TypeDef _") || dump.contains("Existential"),
            "{dump}"
        );
        assert!(
            dump.contains("Existential"),
            "forSome should parse as ExistentialTypeTree: {dump}"
        );
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
        assert!(
            dump.contains("finally") || dump.contains("Literal"),
            "{dump}"
        );
    }

    #[test]
    fn try_finally_without_catch_is_not_dropped() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("ok")
    } finally {
      println("fin")
    }
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Try"), "{dump}");
        assert!(dump.contains("fin"), "parser must keep finally, got {dump}");
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
        assert!(
            dump.contains("Typed"),
            "eta `inc _` should parse as Typed: {dump}"
        );
        assert!(dump.contains("Function"), "{dump}");
        assert!(dump.contains("$anon"), "anonymous class: {dump}");
    }

    #[test]
    fn empty_body_anonymous_new_is_still_anon() {
        let t = parse_ok("object M { val t = new Tag[Int] {} }\n");
        let dump = dump_tree(&t);
        assert!(
            dump.contains("$anon"),
            "new T {{}} must be an anonymous class, not new T: {dump}"
        );
    }

    #[test]
    fn view_bounds_parse() {
        let t = parse_ok(
            r#"
object Main {
  def lt[T <% Ordered[T]](a: T, b: T): Boolean = a < b
  def asInt[T <% Ordered[Int]](x: T): T = x
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("view"),
            "view bound should be kept on TypeDef: {dump}"
        );
        assert!(dump.contains("Ordered"), "{dump}");
        let r = parse_str("object M { def f[T <% Ordered[T] <% Ordered[Int]](x: T): T = x }\n");
        assert!(
            !has_errors(&r.diags),
            "multiple view bounds should parse: {:?}",
            r.diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn context_bounds_and_repeated_parse() {
        let t = parse_ok(
            r#"
object Main {
  def mk[T: ClassTag](n: Int): Array[T] = n
  def q(args: Any*): String = "ok"
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("ctx"),
            "context bound should be kept on TypeDef: {dump}"
        );
        assert!(dump.contains("ClassTag"), "{dump}");
        assert!(
            dump.contains("<repeated>"),
            "T* should wrap as <repeated>: {dump}"
        );
        let r = parse_str("object M { def f[T: Foo: Bar](x: T): T = x }\n");
        assert!(
            !has_errors(&r.diags),
            "multiple context bounds should parse: {:?}",
            r.diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn type_projection_and_variance_parse() {
        let t = parse_ok(
            r#"
trait Foo { type A }
class C[+A](val x: A)
class D[-A](x: A)
object Main {
  def f(n: Foo#A): Foo#A = n
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("SelectFromType #A"), "{dump}");
        assert!(dump.contains("TypeDef +A"), "{dump}");
        assert!(dump.contains("TypeDef -A"), "{dump}");
    }

    #[test]
    fn switch_ascription_parses() {
        let t = parse_ok(
            r#"
import scala.annotation.switch
object Main {
  def f(n: Int): Int = (n: @switch) match {
    case 0 => 1
    case 1 => 2
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("AnnotatedType"),
            "expected @switch ascription: {dump}"
        );
        assert!(dump.contains("Match"), "{dump}");
    }

    #[test]
    fn early_defs_and_infix_type_parse() {
        let t = parse_ok(
            r#"
trait T { val x: Int }
class C extends { val x = 1 } with T
object Main {
  def f(e: Int Either String): Int Either String = e
  def g(m: Map[Int, String]): Map[Int, String] = m
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("ValDef val x"), "early val x: {dump}");
        assert!(dump.contains("Ident T"), "parent T: {dump}");
        assert!(
            dump.contains("Ident Either")
                && dump.contains("Ident Int")
                && dump.contains("Ident String"),
            "infix Either: {dump}"
        );
        assert!(
            dump.contains("Ident Map"),
            "Map[K, V] stays applied: {dump}"
        );
        let r = parse_str(
            r#"
trait T { val x: Int }
class Bad extends { def f = 1 } with T { val x = 1 }
"#,
        );
        assert!(
            has_errors(&r.diags)
                && r.diags
                    .iter()
                    .any(|d| d.message.contains("only concrete field definitions")),
            "illegal early def: {:?}",
            r.diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn new_type_args_then_select_is_term() {
        // nsc: `new Q[Int].enqueue(1)` is `(new Q[Int]).enqueue(1)`, not a
        // type projection `Q[Int].enqueue`.
        let t = parse_ok(
            r#"
class Q[+A] { def enqueue(x: A): Int = 1 }
object Main {
  def main(args: Array[String]): Unit = {
    val n = new Q[Int].enqueue(1)
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("Select enqueue") && dump.contains("AppliedType"),
            "expected term Select on new Q[Int], got {dump}"
        );
        assert!(
            !dump.contains("SelectFromType .enqueue"),
            "`.enqueue` after type args must not be a type path: {dump}"
        );
    }

    #[test]
    fn singleton_compound_and_annotated_types_parse() {
        let t = parse_ok(
            r#"
class C {
  def me: this.type = this
}
object Main {
  val c = new C()
  def id: c.type = c
  def use(x: A with B): Int = 0
  def h(x: Int @unchecked): Int = x
  def f(xs: List[_ <: AnyRef]): Int = 0
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("SingletonType"), "{dump}");
        assert!(dump.contains("CompoundType"), "{dump}");
        assert!(dump.contains("AnnotatedType"), "{dump}");
        assert!(dump.contains("TypeDef _"), "{dump}");
    }

    #[test]
    fn path_dependent_and_refinement_parse() {
        let t = parse_ok(
            r#"
object Main {
  def f(c: Foo { type A = Int }): c.A = c.x
  def g(x: { def foo: Int }): Int = x.foo
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("CompoundType"), "{dump}");
        assert!(dump.contains("TypeDef A"), "{dump}");
        assert!(dump.contains("DefDef foo"), "{dump}");
    }

    #[test]
    fn self_type_parses() {
        let t = parse_ok(
            r#"
trait Foo { def n: Int }
trait Add { self: Foo =>
  def plus(x: Int): Int = x + n
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Trait Add"), "{dump}");
        assert!(dump.contains("DefDef plus"), "{dump}");
    }

    fn find_val<'a>(t: &'a Tree, want: &str) -> Option<&'a Tree> {
        match &t.kind {
            TreeKind::PackageDef { stats, .. } | TreeKind::Block { stats, .. } => {
                for s in stats {
                    if let Some(v) = find_val(s, want) {
                        return Some(v);
                    }
                }
                None
            }
            TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
                for s in &impl_.body {
                    if let Some(v) = find_val(s, want) {
                        return Some(v);
                    }
                }
                None
            }
            TreeKind::ValDef { name, .. } if name == want => Some(t),
            TreeKind::DefDef { vparamss, rhs, .. } => {
                for c in vparamss {
                    for p in c {
                        if let Some(v) = find_val(p, want) {
                            return Some(v);
                        }
                    }
                }
                find_val(rhs, want)
            }
            _ => None,
        }
    }

    #[test]
    fn private_this_and_protected_qual_parse() {
        let t = parse_ok(
            r#"
class C {
  private[this] val secret: Int = 1
  protected[C] val vis: Int = 2
}
"#,
        );
        let secret = find_val(&t, "secret").expect("secret");
        let vis = find_val(&t, "vis").expect("vis");
        match &secret.kind {
            TreeKind::ValDef { mods, .. } => {
                assert!(mods.flags.contains(Flags::PRIVATE), "{mods:?}");
                assert!(mods.flags.contains(Flags::LOCAL), "{mods:?}");
                assert!(mods.private_within.is_none(), "{mods:?}");
            }
            other => panic!("{other:?}"),
        }
        match &vis.kind {
            TreeKind::ValDef { mods, .. } => {
                assert!(mods.flags.contains(Flags::PROTECTED), "{mods:?}");
                assert_eq!(mods.private_within.as_deref(), Some("C"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tailrec_and_deprecated_annotations_parse() {
        let t = parse_ok(
            r#"
object M {
  @tailrec
  def sum(n: Int, acc: Int): Int = if (n <= 0) acc else sum(n - 1, acc + n)
  @deprecated("old")
  def f(): Int = 1
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("DefDef sum"), "{dump}");
        fn find_def<'a>(t: &'a Tree, want: &str) -> Option<&'a Tree> {
            match &t.kind {
                TreeKind::PackageDef { stats, .. } => stats.iter().find_map(|s| find_def(s, want)),
                TreeKind::ModuleDef { impl_, .. } | TreeKind::ClassDef { impl_, .. } => {
                    impl_.body.iter().find_map(|s| find_def(s, want))
                }
                TreeKind::DefDef { name, .. } if name == want => Some(t),
                _ => None,
            }
        }
        let sum = find_def(&t, "sum").expect("sum");
        match &sum.kind {
            TreeKind::DefDef { mods, .. } => {
                assert_eq!(mods.annotations.len(), 1, "{mods:?}");
                assert_eq!(mods.annotations[0].annotation_path(), "tailrec");
            }
            other => panic!("{other:?}"),
        }
        let f = find_def(&t, "f").expect("f");
        match &f.kind {
            TreeKind::DefDef { mods, .. } => {
                assert_eq!(mods.annotations.len(), 1, "{mods:?}");
                assert_eq!(mods.annotations[0].annotation_path(), "deprecated");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn inline_volatile_transient_parse() {
        let t = parse_ok(
            r#"
object M {
  @inline def f(): Int = 1
  @noinline def g(): Int = 2
  @volatile var x: Int = 0
  @transient var y: Int = 1
  @native def n(): Int
}
"#,
        );
        fn find_member<'a>(t: &'a Tree, want: &str) -> Option<&'a Tree> {
            match &t.kind {
                TreeKind::PackageDef { stats, .. } => {
                    stats.iter().find_map(|s| find_member(s, want))
                }
                TreeKind::ModuleDef { impl_, .. } | TreeKind::ClassDef { impl_, .. } => {
                    impl_.body.iter().find_map(|s| match &s.kind {
                        TreeKind::DefDef { name, .. } | TreeKind::ValDef { name, .. }
                            if name == want =>
                        {
                            Some(s)
                        }
                        _ => None,
                    })
                }
                _ => None,
            }
        }
        match &find_member(&t, "f").expect("f").kind {
            TreeKind::DefDef { mods, .. } => {
                assert_eq!(mods.annotations[0].annotation_path(), "inline");
            }
            other => panic!("{other:?}"),
        }
        match &find_member(&t, "x").expect("x").kind {
            TreeKind::ValDef { mods, .. } => {
                assert!(mods.flags.contains(Flags::VOLATILE), "{mods:?}");
            }
            other => panic!("{other:?}"),
        }
        match &find_member(&t, "y").expect("y").kind {
            TreeKind::ValDef { mods, .. } => {
                assert!(mods.flags.contains(Flags::TRANSIENT), "{mods:?}");
            }
            other => panic!("{other:?}"),
        }
        match &find_member(&t, "n").expect("n").kind {
            TreeKind::DefDef { mods, .. } => {
                assert!(mods.flags.contains(Flags::NATIVE), "{mods:?}");
                assert_eq!(mods.annotations[0].annotation_path(), "native");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn new_then_select_assign_is_two_stats() {
        let t = parse_ok(
            r#"
class Box { var x: Int = 0 }
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box
    b.x = 3
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("Assign") && dump.contains("Select x"),
            "expected `b.x = 3` after `new Box`, got {dump}"
        );
    }

    #[test]
    fn java_override_and_deprecated_annotations_parse() {
        let t = parse_ok(
            r#"
class A { def tag: String = "a" }
class B extends A {
  @Override
  def tag: String = "b"
  @Deprecated
  def old(): Int = 1
}
"#,
        );
        fn find_class_def<'a>(t: &'a Tree, cls: &str, want: &str) -> Option<&'a Tree> {
            match &t.kind {
                TreeKind::PackageDef { stats, .. } => {
                    stats.iter().find_map(|s| find_class_def(s, cls, want))
                }
                TreeKind::ClassDef { name, impl_, .. } if name == cls => {
                    impl_.body.iter().find_map(|s| match &s.kind {
                        TreeKind::DefDef { name, .. } if name == want => Some(s),
                        _ => None,
                    })
                }
                TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
                    impl_.body.iter().find_map(|s| find_class_def(s, cls, want))
                }
                _ => None,
            }
        }
        let tag = find_class_def(&t, "B", "tag").expect("B.tag");
        match &tag.kind {
            TreeKind::DefDef { mods, .. } => {
                assert_eq!(mods.annotations.len(), 1, "{mods:?}");
                assert_eq!(mods.annotations[0].annotation_path(), "Override");
            }
            other => panic!("{other:?}"),
        }
        let old = find_class_def(&t, "B", "old").expect("B.old");
        match &old.kind {
            TreeKind::DefDef { mods, .. } => {
                assert_eq!(mods.annotations.len(), 1, "{mods:?}");
                assert_eq!(mods.annotations[0].annotation_path(), "Deprecated");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn do_while_parses() {
        let t = parse_ok(
            r#"
object M {
  def f(): Int = {
    var i: Int = 0
    do { i = i + 1 } while (i < 3)
    i
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("DoWhile"), "{dump}");
    }

    #[test]
    fn postfix_select_same_line() {
        let t = parse_ok(
            r#"
object M {
  def f(xs: List[Int]): Any = xs toList
  def g(): Int = 42 bang
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Select toList postfix"), "{dump}");
        assert!(dump.contains("Select bang postfix"), "{dump}");
        assert!(
            !dump.contains("Select + postfix"),
            "infix plus must not be postfix: {dump}"
        );
    }

    #[test]
    fn xml_literal_elem_text_splice_desugars() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n = 1
    val x = <a>t{n}</a>
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Elem"), "expected Elem constructor: {dump}");
        assert!(dump.contains("Text"), "expected Text node: {dump}");
        assert!(dump.contains("Atom"), "expected Atom splice: {dump}");
        assert!(dump.contains("Null"), "{dump}");
        assert!(dump.contains("TopScope"), "{dump}");
    }

    #[test]
    fn xml_self_closing_desugars() {
        let t = parse_ok("object M { val x = <a/> }\n");
        let dump = dump_tree(&t);
        assert!(dump.contains("Elem"), "{dump}");
    }

    #[test]
    fn xml_attributes_desugar() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val e = "1"
    val x = <a b={e} c="t"/>
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("UnprefixedAttribute"),
            "expected UnprefixedAttribute: {dump}"
        );
        assert!(dump.contains("Elem"), "{dump}");
    }

    #[test]
    fn xml_namespaces_desugar() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val e = "1"
    val x = <a xmlns:p="u" p:b={e} c="t"/>
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(
            dump.contains("PrefixedAttribute"),
            "expected PrefixedAttribute: {dump}"
        );
        assert!(
            dump.contains("NamespaceBinding"),
            "expected NamespaceBinding: {dump}"
        );
        assert!(
            dump.contains("UnprefixedAttribute"),
            "expected UnprefixedAttribute: {dump}"
        );
        assert!(dump.contains("Elem"), "{dump}");
    }

    #[test]
    fn xml_prefixed_element_desugars() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = <p:a xmlns:p="u"/>
    val y = <p:b xmlns:p="u">t</p:b>
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Elem"), "{dump}");
        assert!(
            dump.contains("NamespaceBinding"),
            "expected NamespaceBinding: {dump}"
        );
        // prefix argument is Lit.String("p"), not null.
        assert!(
            dump.contains("Literal \"p\""),
            "expected prefixed Elem prefix \"p\": {dump}"
        );
    }

    #[test]
    fn xml_comments_cdata_pi_desugar() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a = <a><!--c--></a>
    val b = <a><![CDATA[x]]></a>
    val c = <a><?pi t?></a>
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("Comment"), "expected Comment: {dump}");
        assert!(dump.contains("PCData"), "expected PCData: {dump}");
        assert!(dump.contains("ProcInstr"), "expected ProcInstr: {dump}");
    }

    #[test]
    fn xml_entities_desugar() {
        let t = parse_ok(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a = <a>&amp;</a>
    val b = <a>&#65;</a>
    val c = <a>&#x42;</a>
  }
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("EntityRef"), "expected EntityRef: {dump}");
        assert!(dump.contains("amp"), "expected entity name amp: {dump}");
    }

    #[test]
    fn xml_unknown_entity_is_unimplemented() {
        let r = parse_str("object M { val x = <a>&notanentity;</a> }\n");
        assert!(
            r.diags
                .iter()
                .any(|d| d.message.contains("XML entity") || d.message.contains("unknown")),
            "expected unknown XML entity diagnostic, got {:?}",
            r.diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn multi_val_def_flattens() {
        let t = parse_ok(
            r#"
object Color {
  val Red, Blue = 1
}
"#,
        );
        let dump = dump_tree(&t);
        assert!(dump.contains("ValDef val Red"), "{dump}");
        assert!(dump.contains("ValDef val Blue"), "{dump}");
    }
}
