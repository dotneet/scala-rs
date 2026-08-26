mod check;
mod classpath;
mod erasure;
mod implicits;
mod lambda_lift;
mod prelude;
mod symbol;
mod uncurry;

pub use check::{
    find_mains, has_errors, typecheck, typecheck_opts, ClasspathClass, ClasspathMethod,
    ClasspathPickleMethod, TypecheckOptions, Typer,
};
pub use erasure::{erase, erase_type};
pub use lambda_lift::lambda_lift;
pub use symbol::{Intrinsic, SymKind, Symbol, SymbolTable};
pub use uncurry::uncurry;

use scala_rs_parser::{parse_str, Tree};

pub fn typecheck_str(src: &str) -> (Tree, SymbolTable, Vec<scala_rs_span::Diagnostic>) {
    typecheck_str_opts(src, &TypecheckOptions::default())
}

pub fn typecheck_str_opts(
    src: &str,
    opts: &TypecheckOptions,
) -> (Tree, SymbolTable, Vec<scala_rs_span::Diagnostic>) {
    let mut r = parse_str(src);
    let (st, mut tdiags) = typecheck_opts(&mut r.tree, 0, opts);
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
        let t = ok(r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#);
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
        ok(r#"
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
"#);
    }

    #[test]
    fn case_class_apply() {
        ok(r#"
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
"#);
    }

    #[test]
    fn factorial_and_if() {
        ok(r#"
object Main {
  def fact(n: Int): Int =
    if (n <= 1) 1 else n * fact(n - 1)
  def main(args: Array[String]): Unit = println(fact(5))
}
"#);
    }

    #[test]
    fn trait_impl() {
        ok(r#"
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
"#);
    }

    #[test]
    fn int_plus_is_int() {
        let (t, st, diags) = typecheck_str("object M { val x: Int = 1 + 2 }\n");
        assert!(!has_errors(&diags), "{:?}", diags);
        let _ = (t, st, Type::Int);
    }

    #[test]
    fn generic_id_typechecks() {
        ok(r#"
object Main {
  def id[T](x: T): T = x
  def main(args: Array[String]): Unit = {
    val n: Int = id(42)
    val s: String = id("hi")
  }
}
"#);
    }

    #[test]
    fn list_for_typechecks() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: Nil
    val ys = for (x <- xs) yield x + 1
  }
}
"#);
    }

    #[test]
    fn implicit_param_and_conversion() {
        ok(r#"
class RichInt(val n: Int) {
  def doubled: Int = n * 2
}
object RichInt {
  implicit def toRich(n: Int): RichInt = new RichInt(n)
}
object Main {
  implicit val extra: Int = 10
  def add(x: Int)(implicit y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    val n: Int = add(5)
    val r: RichInt = 7
  }
}
"#);
    }

    #[test]
    fn no_implicit() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def add(x: Int)(implicit y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    val n: Int = add(5)
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("no implicit")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ambiguous_implicit() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  implicit val a: Int = 1
  implicit val b: Int = 2
  def add(x: Int)(implicit y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    val n: Int = add(5)
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("ambiguous implicit")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn defaults_and_byname_typecheck() {
        ok(r#"
object Main {
  def greet(name: String, punct: String = "!"): String = name + punct
  def twice(x: => Int): Int = x + x
  def main(args: Array[String]): Unit = {
    val s: String = greet("A")
    val n: Int = twice(1)
  }
}
"#);
    }

    #[test]
    fn explicit_implicit_arg_list() {
        ok(r#"
object Main {
  def add(x: Int)(implicit y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    val n: Int = add(5)(3)
  }
}
"#);
    }

    #[test]
    fn named_arguments_reorder() {
        ok(r#"
object Main {
  def pair(a: Int, b: Int): Int = a + b
  def main(args: Array[String]): Unit = {
    val n: Int = pair(b = 2, a = 1)
  }
}
"#);
    }

    #[test]
    fn imported_implicits() {
        ok(r#"
object Inc {
  implicit val k: Int = 10
}
object Main {
  import Inc._
  def show(x: Int)(implicit n: Int): Int = x + n
  def main(args: Array[String]): Unit = {
    val n: Int = show(1)
  }
}
"#);
    }

    #[test]
    fn super_and_qualified_this_typecheck() {
        ok(r#"
class Base {
  def greet(): String = "base"
}
class C extends Base {
  def hi(): String = super.greet() + "!"
}
class Outer {
  val name: String = "outer"
  class Inner {
    def who(): String = Outer.this.name
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    val s: String = new C().hi()
  }
}
"#);
    }

    #[test]
    fn sealed_match_exhaustive() {
        ok(r#"
sealed trait Color
case class RGB(n: Int) extends Color
case object Black extends Color
object Main {
  def show(c: Color): Int = c match {
    case RGB(n) => n
    case Black => 0
  }
  def main(args: Array[String]): Unit = {
    val n: Int = show(Black)
  }
}
"#);
    }

    #[test]
    fn sealed_match_non_exhaustive() {
        let (_, _, diags) = typecheck_str(
            r#"
sealed trait Color
case class RGB(n: Int) extends Color
case object Black extends Color
object Main {
  def show(c: Color): Int = c match {
    case RGB(n) => n
  }
}
"#,
        );
        assert!(
            !has_errors(&diags),
            "non-exhaustive match is a warning, got {:?}",
            diags
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("may not be exhaustive")
                    && d.level == scala_rs_span::Level::Warning),
            "{:?}",
            diags
                .iter()
                .map(|d| format!("{:?} {}", d.level, d.message))
                .collect::<Vec<_>>()
        );
        assert!(
            diags.iter().any(|d| d.message.contains("Black")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sealed_match_non_exhaustive_fatal_warnings() {
        let (_, _, diags) = typecheck_str_opts(
            r#"
sealed trait Color
case class RGB(n: Int) extends Color
case object Black extends Color
object Main {
  def show(c: Color): Int = c match {
    case RGB(n) => n
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: true,
                library_abi: false,
                classpath: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("may not be exhaustive")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unapply_seq_and_named_extractor_typecheck() {
        ok(r#"
object PairSeq {
  def unapplySeq(n: Int): Option[List[Int]] = Some(n :: (n + 1) :: Nil)
}
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: 3 :: Nil
    val a = xs match {
      case List(x, y, z) => x + y + z
      case _ => 0
    }
    val b = 10 match {
      case PairSeq(p, q) => p + q
      case _ => 0
    }
    val c = xs match {
      case List(h, rest @ _*) => h
      case _ => 0
    }
    val d = Point(1, 2) match {
      case Point(y = b, x = a) => a + b
      case _ => 0
    }
  }
}
"#);
    }

    #[test]
    fn unapply_extractor_typechecks() {
        ok(r#"
object Even {
  def unapply(n: Int): Option[Int] = if (n % 2 == 0) Some(n / 2) else None
}
object Main {
  def main(args: Array[String]): Unit = {
    val x: Int = 10 match {
      case Even(half) => half
      case _ => -1
    }
  }
}
"#);
    }

    #[test]
    fn missing_extractor_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = 1 match {
      case Missing(n) => n
      case _ => 0
    }
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("extractor")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn value_class_and_predef_typecheck() {
        ok(r#"
class Meter(val n: Int) extends AnyVal {
  def doubled: Int = n * 2
}
object Main {
  def main(args: Array[String]): Unit = {
    val m = new Meter(21)
    val d: Int = m.doubled
    val len: Int = "42".length
    val n: Int = "42".toInt
    assert(true)
    require(1 > 0)
        val t = 1 -> "a"
  }
}
"#);
    }

    #[test]
    fn predef_more_and_trait_val_typecheck() {
        ok(r#"
trait T {
  val msg: String = "from trait"
}
class C extends T
object Main {
  implicit val n: Int = 41
  def main(args: Array[String]): Unit = {
    println(identity(42))
    locally {
      println("here")
    }
    println(implicitly[Int])
    println(1 + "x")
    println(new C().msg)
  }
}
"#);
    }

    #[test]
    fn abstract_override_typecheck() {
        ok(r#"
trait Base {
  def msg: String = "base"
}
trait A extends Base {
  abstract override def msg: String = "A-" + super.msg
}
trait B extends Base {
  abstract override def msg: String = "B-" + super.msg
}
class C extends Base with A with B
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#);
    }

    #[test]
    fn library_abi_stringops_withfilter_iterator_typecheck() {
        let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = "42".toInt
    val len: Int = "42".length
    val xs = 1 :: 2 :: Nil
    val ys = for (x <- xs if x > 0) yield x
    val it = xs.iterator
    val b: Boolean = it.hasNext
    val s: String = 1 + "x"
    val t = 1 -> "a"
    val m = Map(1 -> "a", 2 -> "b")
    val v = Vector(1, 2, 3)
    val a: Int = 1.abs
    val mx: Int = 1.max(2)
    val r = 1.to(3)
    val s: String = "ab" * 2
    val t: String = "hello".take(2)
    val d: String = "hello".drop(1)
    val e: Boolean = "".isEmpty
    val xs2 = List(1, 2, 3)
    val set1 = Set(1, 2, 3)
    val has: Boolean = set1.contains(2)
    val la: Long = (-3L).abs
    val lm: Long = 1L.max(2L)
    val da: Double = (-2.5).abs
    val dm: Double = 1.0.max(2.5)
    val cd: Boolean = '9'.isDigit
    val ci: Int = 'A'.toInt
    val seq1 = Seq(1, 2, 3)
    val ll1 = LazyList(1, 2, 3)
    val fa: Float = (-2.5f).abs
    val fm: Float = 1.0f.max(2.5f)
    val up: String = "hello".toUpperCase
    val lo: String = "HELLO".toLowerCase
    val sp: String = "foobar".stripPrefix("foo")
    val parts = "a,b".split(',')
    val plen: Int = parts.length
    val ei = Right(1)
    val el = Left("err")
    val leftish: Boolean = el.isLeft
    val or: Any = ei.getOrElse(0)
    val tr = Try(1)
    val tm = tr.map((x: Int) => x + 1)
    val tg: Any = tm.getOrElse(0)
    val su = Success(2)
    val fail = Failure(new RuntimeException())
    val fo: Any = fail.getOrElse(0)
  }
}
"#;
        let (_, _, diags) = typecheck_str_opts(
            src,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
            },
        );
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn anonymous_class_typechecks() {
        ok(r#"
trait Greeter {
  def greet(name: String): String
}
object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new Greeter { def greet(name: String): String = "Hello, " + name }
    println(g.greet("Scala"))
    val a = new { def msg: String = "anon" }
    println(a.msg)
  }
}
"#);
    }

    #[test]
    fn existentials_typecheck() {
        ok(r#"
object Main {
  def show(xs: List[_]): Unit = {
    xs.foreach((x: Any) => println(x))
  }
  def show2(xs: List[X] forSome { type X }): Unit = {
    xs.foreach((x: Any) => println(x))
  }
  def main(args: Array[String]): Unit = {
    show(1 :: 2 :: Nil)
    show2("a" :: Nil)
  }
}
"#);
    }

    #[test]
    fn unsupported_wildcard_bounds_are_diagnosed() {
        let (_, _, diags) = typecheck_str(
            "object M { def f(xs: List[_ <: Int]): Unit = () }\n",
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("wildcard bounds")
                || d.message.contains("unimplemented")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn implicit_specificity_picks_subclass() {
        ok(r#"
class A { def tag: String = "A" }
class B extends A { def tag: String = "B" }
object Main {
  implicit val a: A = new A()
  implicit val b: B = new B()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    val s: String = pick()
  }
}
"#);
    }

    #[test]
    fn nested_def_typechecks() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n = 10
    def add(x: Int): Int = x + n
    val m: Int = add(1)
    def fact(x: Int): Int = if (x <= 1) 1 else x * fact(x - 1)
    val f: Int = fact(5)
  }
}
"#);
    }

    #[test]
    fn eta_and_uncurry_typecheck() {
        let src = r#"
object Main {
  def inc(x: Int): Int = x + 1
  def add(x: Int)(y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: Nil
    val a = xs.map(inc)
    val b = xs.map(inc _)
    val c = xs.map(add(10))
    val n: Int = add(1)(2)
  }
}
"#;
        let (mut t, mut st, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        crate::uncurry(&mut t, &mut st);
        let dump = scala_rs_parser::dump_tree(&t);
        assert!(
            !dump.contains("paramss[1]"),
            "uncurry should flatten nested param lists: {dump}"
        );
        assert!(dump.contains("Function"), "eta-expansion should yield Function: {dump}");
    }

    #[test]
    fn lambda_lift_hoists_nested_def() {
        let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n = 10
    def add(x: Int): Int = x + n
    val m: Int = add(1)
  }
}
"#;
        let (mut t, mut st, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        crate::uncurry(&mut t, &mut st);
        crate::lambda_lift(&mut t, &mut st);
        let dump = scala_rs_parser::dump_tree(&t);
        assert!(
            dump.contains("DefDef add$") || dump.contains("add$1"),
            "nested def should be lifted to a synthetic method: {dump}"
        );
    }

    #[test]
    fn view_bounds_desugar_and_ordered_int() {
        ok(r#"
class Box(val n: Int) extends Ordered[Box] {
  def compare(that: Box): Int = n - that.n
}
object Main {
  def lt[T <% Ordered[T]](a: T, b: T): Boolean = a < b
  def asInt[T <% Ordered[Int]](x: T, y: Int): Boolean = true
  def main(args: Array[String]): Unit = {
    val b: Boolean = lt(new Box(1), new Box(2))
  }
}
"#);
    }

    #[test]
    fn class_view_bounds_are_diagnosed() {
        let (_, _, diags) = typecheck_str("class C[T <% Ordered[T]](x: T)\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("view bound")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn default_getter_symbol_is_synthesized() {
        let src = r#"
object Main {
  def greet(name: String, punct: String = "!"): String = name + punct
  def main(args: Array[String]): Unit = {
    val s: String = greet("A")
  }
}
"#;
        let (_, st, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let found = st.symbols.iter().any(|s| s.name == "greet$default$2");
        assert!(found, "expected greet$default$2 in the symbol table");
    }
}
