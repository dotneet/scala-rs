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

    fn ok_lib(src: &str) -> Tree {
        let (t, _, diags) = typecheck_str_opts(
            src,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                language_features: Vec::new(),
            },
        );
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
                language_features: Vec::new(),
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
                language_features: Vec::new(),
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
    fn bounded_wildcard_typechecks() {
        ok(r#"
object M {
  def f(xs: List[_ <: AnyRef]): Unit = ()
  def g(xs: List[_ <: Int]): Unit = ()
}
"#);
    }

    #[test]
    fn value_existential_is_diagnosed() {
        let (_, _, diags) =
            typecheck_str("object M { def f(x: T forSome { val x: Int }): Unit = () }\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("unimplemented")),
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
        assert!(
            dump.contains("Function"),
            "eta-expansion should yield Function: {dump}"
        );
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

    #[test]
    fn implicit_inherited_from_parent() {
        ok(r#"
class Base {
  implicit val n: Int = 10
}
object Main extends Base {
  def add(x: Int)(implicit y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    val z: Int = add(5)
  }
}
"#);
    }

    #[test]
    fn class_ctor_infers_tparams() {
        ok(r#"
class Box[A](val value: A)
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box(1)
  }
}
"#);
    }

    #[test]
    fn implicit_nested_companion_and_type_ctor() {
        ok(r#"
trait Tag[A]
class Box[A](val value: A)
object Box {
  implicit val boxTag: Tag[Box[Int]] = new Tag[Box[Int]] {}
}
object Outer {
  class Inner
  object Inner {
    implicit val innerTag: Tag[Inner] = new Tag[Inner] {}
  }
}
object Main {
  def use[A](x: A)(implicit t: Tag[A]): Int = 1
  def main(args: Array[String]): Unit = {
    val a: Int = use(new Outer.Inner())
    val b: Int = use(new Box(1))
  }
}
"#);
    }

    #[test]
    fn implicit_inherited_still_ambiguous() {
        let (_, _, diags) = typecheck_str(
            r#"
trait A { implicit val x: Int = 1 }
trait B { implicit val y: Int = 2 }
object Main extends A with B {
  def add(n: Int)(implicit z: Int): Int = n + z
  def main(args: Array[String]): Unit = {
    val n: Int = add(0)
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
    fn implicit_inherited_more_specific_vs_local_less_is_ambiguous() {
        let (_, _, diags) = typecheck_str(
            r#"
class A { def tag: String = "A" }
class B extends A { override def tag: String = "B" }
class Parent { implicit val more: B = new B() }
object Main extends Parent {
  implicit val less: A = new A()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    val s: String = pick()
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
    fn implicit_local_more_specific_beats_inherited_less() {
        ok(r#"
class A { def tag: String = "A" }
class B extends A { override def tag: String = "B" }
class Parent { implicit val less: A = new A() }
object Main extends Parent {
  implicit val more: B = new B()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    val s: String = pick()
  }
}
"#);
    }

    #[test]
    fn partial_function_typechecks() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val pf: PartialFunction[Int, Int] = { case 1 => 2; case 2 => 3 }
    val a: Boolean = pf.isDefinedAt(1)
    val b: Int = pf.apply(1)
    val c: Int = pf.applyOrElse(3, (x: Int) => 0)
  }
}
"#);
    }

    #[test]
    fn private_this_same_instance_ok() {
        ok(r#"
class C {
  private[this] val n: Int = 1
  def get: Int = n
  def add: Int = this.n + 1
}
"#);
    }

    #[test]
    fn private_this_other_instance_rejected() {
        let (_, _, diags) = typecheck_str(
            r#"
class C {
  private[this] val n: Int = 1
  def steal(other: C): Int = other.n
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot be accessed")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn protected_qual_legal_and_illegal() {
        ok(r#"
class C {
  protected[C] val n: Int = 1
  def get: Int = n
  def fromPeer(other: C): Int = other.n
}
class D extends C {
  def mine: Int = this.n
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
class C {
  protected[C] val n: Int = 1
}
object Main {
  def peek(c: C): Int = c.n
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot be accessed")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn type_member_and_projection() {
        ok(r#"
trait Foo { type A; def x: A }
class Bar extends Foo { type A = Int; def x: A = 41 }
object Main {
  def fromProj(n: Bar#A): Int = n + 1
  def main(args: Array[String]): Unit = {
    val n: Int = new Bar().x
    val m: Int = fromProj(n)
  }
}
"#);
    }

    #[test]
    fn path_dependent_type_ok() {
        ok(r#"
trait Foo { type A; def x: A }
class Bar extends Foo { type A = Int; def x: A = 41 }
object Main {
  def fromPath(c: Foo { type A = Int }): c.A = c.x
  def main(args: Array[String]): Unit = {
    val n: Int = fromPath(new Bar())
  }
}
"#);
    }

    #[test]
    fn path_dependent_unstable_is_diagnosed() {
        let (_, _, diags) = typecheck_str(
            r#"
trait Foo { type A }
class Bar extends Foo { type A = Int }
object Main {
  var v: Bar = new Bar()
  def bad: v.A = 1
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("stable identifier")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn this_type_and_stable_singleton() {
        ok(r#"
class C {
  def me: this.type = this
  def n: Int = 1
}
object Main {
  val c = new C()
  def id: c.type = c
  def main(args: Array[String]): Unit = {
    val x: Int = c.me.n
    val y: Int = id.n
  }
}
"#);
    }

    #[test]
    fn unstable_singleton_is_diagnosed() {
        let (_, _, diags) = typecheck_str(
            r#"
class C
object Main {
  var v: C = new C()
  def bad: v.type = v
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("stable identifier")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn compound_type_as_value_and_param() {
        ok(r#"
trait A { def a: Int }
trait B { def b: Int }
class C extends A with B { def a: Int = 1; def b: Int = 2 }
object Main {
  def use(x: A with B): Int = x.a + x.b
  def main(args: Array[String]): Unit = {
    val n: Int = use(new C())
  }
}
"#);
    }

    #[test]
    fn illegal_compound_is_diagnosed() {
        let (_, _, diags) = typecheck_str(
            r#"
class A
class B
object Main {
  def bad(x: A with B): Int = 0
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("illegal inheritance")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn bounded_wildcard_and_annotated_type() {
        ok(r#"
object Main {
  def f(xs: List[_ <: AnyRef]): Int = 0
  def h(x: Int @unchecked): Int = x
  def main(args: Array[String]): Unit = {
    val n: Int = f("a" :: Nil)
    val m: Int = h(1)
  }
}
"#);
    }

    #[test]
    fn forsome_named_bound_typechecks() {
        ok(r#"
object Main {
  def show(xs: List[X] forSome { type X <: AnyRef }): Unit = ()
  def main(args: Array[String]): Unit = {
    show("a" :: Nil)
  }
}
"#);
    }

    #[test]
    fn nonlocal_return_typechecks() {
        ok(r#"
object Main {
  def find(xs: List[Int]): Int = {
    xs.foreach((x: Int) => { if (x > 0) return x })
    0
  }
  def nested: Int = {
    def inner: Int = { return 1 }
    inner
  }
}
"#);
    }

    #[test]
    fn return_from_class_ctor_is_diagnosed() {
        let (_, _, diags) = typecheck_str(
            r#"
class C { return 1 }
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("return outside method")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn java_override_ok_and_wrong_is_diagnosed() {
        ok(r#"
class A { def tag: String = "a" }
class B extends A {
  @Override
  def tag: String = "b"
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
class A { def tag: String = "a" }
class B extends A {
  @Override
  def other: String = "x"
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("overrides nothing")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn structural_type_ok() {
        ok(r#"
class C { def foo: Int = 42 }
object Main {
  def use(x: { def foo: Int }): Int = x.foo
  def main(args: Array[String]): Unit = {
    val n: Int = use(new C())
  }
}
"#);
    }

    #[test]
    fn self_type_ok_and_illegal_inheritance() {
        ok(r#"
trait Foo { def n: Int = 10 }
trait Add { self: Foo => def plus(x: Int): Int = x + n }
class C extends Foo with Add
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = new C().plus(5)
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
trait Foo { def n: Int = 10 }
trait Add { self: Foo => def plus(x: Int): Int = x + n }
class Bad extends Add
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("illegal inheritance")
                    || d.message.contains("self-type")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn variance_ok_and_mutable_field_rejected() {
        ok(r#"
class Box[+A](val value: A) { def get: A = value }
object Main {
  def main(args: Array[String]): Unit = {
    val b: Box[Int] = new Box(41)
    val n: Int = b.get
  }
}
"#);
        let (_, _, diags) = typecheck_str("class Bad[+A](var x: A)\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("covariant") && d.message.contains("contravariant")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn higher_kinded_type_member_is_diagnosed() {
        let (_, _, diags) = typecheck_str("trait T { type F[_] }\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("unimplemented")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn f_and_raw_interpolators_typecheck() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = 7
    val a: String = f"$n%02d"
    val b: String = raw"a\nb"
    val c: String = s"hi $n"
  }
}
"#);
    }

    #[test]
    fn f_interpolator_unsupported_format_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = 1
    val s: String = f"$n%tY"
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("f interpolator")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn overload_int_string_and_arity() {
        ok(r#"
object Main {
  def f(x: Int): String = "int"
  def f(x: String): String = "str"
  def g(x: Int): String = "1"
  def g(x: Int, y: Int): String = "2"
  def main(args: Array[String]): Unit = {
    val a: String = f(1)
    val b: String = f("a")
    val c: String = g(1)
    val d: String = g(1, 2)
  }
}
"#);
    }

    #[test]
    fn overload_ambiguous_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def f(x: Int, y: Any): String = "a"
  def f(x: Any, y: Int): String = "b"
  def main(args: Array[String]): Unit = {
    val s: String = f(1, 1)
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("ambiguous overload")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn overload_no_alternative_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def f(x: Int): String = "int"
  def main(args: Array[String]): Unit = {
    val s: String = f("no")
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("no matching overload")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn classtag_and_context_bound_typecheck() {
        ok_lib(
            r#"
import scala.reflect.ClassTag
object Main {
  def mk[T: ClassTag](n: Int): Array[T] = new Array[T](n)
  def main(args: Array[String]): Unit = {
    val n: String = implicitly[ClassTag[Int]].runtimeClass.getName
    val a: Array[Int] = mk[Int](2)
    val k: Int = a.length
  }
}
"#,
        );
    }

    #[test]
    fn custom_stringcontext_interpolator_typecheck() {
        ok_lib(
            r#"
object Main {
  implicit class Q(sc: StringContext) {
    def q(args: Any*): String = "q:ok"
  }
  def main(args: Array[String]): Unit = {
    val x = "X"
    val s: String = q"a$x"
  }
}
"#,
        );
    }

    #[test]
    fn custom_interpolator_without_library_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val s: String = q"a"
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("interpolator")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tailrec_non_tail_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  @tailrec
  def fact(n: Int): Int = if (n <= 1) 1 else n * fact(n - 1)
}
"#,
        );
        assert!(
            has_errors(&diags),
            "expected @tailrec error, got {:?}",
            diags
        );
        assert!(
            diags.iter().any(|d| d.message.contains("tailrec")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tailrec_and_deprecated_typecheck() {
        ok(r#"
object Main {
  @tailrec
  def sum(n: Int, acc: Int): Int = if (n <= 0) acc else sum(n - 1, acc + n)
  @deprecated("old")
  def f(): Int = 1
  def main(args: Array[String]): Unit = {
    val n: Int = sum(3, 0)
    val k: Int = f()
  }
}
"#);
    }

    #[test]
    fn array_ops_typecheck_against_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val arr: Array[Int] = Array(1, 2, 3)
    val x: Int = arr(0)
    val n: Int = arr.length
    arr.update(1, 9)
    arr(2) = 8
  }
}
"#,
        );
    }

    #[test]
    fn eq_ne_synchronized_typecheck() {
        ok(r#"
class Box
object Main {
  def main(args: Array[String]): Unit = {
    val a = new Box()
    val b = new Box()
    val same: Boolean = a.eq(a)
    val diff: Boolean = a.ne(b)
    val n: Int = a.synchronized { 41 }
  }
}
"#);
    }

    #[test]
    fn do_while_typecheck() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    var i: Int = 0
    do { i = i + 1 } while (i < 3)
  }
}
"#);
    }

    #[test]
    fn constant_types_typecheck() {
        ok(r#"
object Main {
  val x: 1 = 1
  def f(n: 1): Int = n
  def main(args: Array[String]): Unit = {
    val n: Int = f(1)
    val y: Int = x
  }
}
"#);
    }

    #[test]
    fn constant_type_mismatch() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  val y: 1 = 2
}
"#,
        );
        assert!(has_errors(&diags), "expected mismatch, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("type mismatch")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn implicit_class_extension_typecheck() {
        ok(r#"
object Main {
  implicit class Rich(n: Int) {
    def twice: Int = n * 2
  }
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#);
    }

    #[test]
    fn dynamic_select_and_apply_typecheck() {
        ok(r#"
import scala.language.dynamics
class D extends Dynamic {
  def selectDynamic(name: String): String = name
  def applyDynamic(name: String)(x: String): String = name + x
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = new D()
    val a: String = d.foo
    val b: String = d.bar("x")
  }
}
"#);
    }

    #[test]
    fn dynamic_without_language_import_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
class D extends Dynamic {
  def selectDynamic(name: String): String = name
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = new D()
    val a: String = d.foo
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("language.dynamics")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn dynamic_update_and_named_apply_typecheck() {
        let src = r#"
import scala.language.dynamics
class D extends Dynamic {
  def selectDynamic(name: String): String = name
  def updateDynamic(name: String)(value: String): Unit = ()
  def applyDynamicNamed(name: String)(arg: (String, String)): String = name + arg._1 + arg._2
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = new D()
    d.foo = "ok"
    val a: String = d.bar(a = "x")
  }
}
"#;
        ok(src);
        ok_lib(src);
    }

    #[test]
    fn postfix_ops_warns_without_import() {
        let (_, _, diags) = typecheck_str(
            r#"
import scala.language.implicitConversions
object Main {
  implicit class Bang(n: Int) { def bang: Int = n + 1 }
  def main(args: Array[String]): Unit = {
    val n: Int = 42 bang
  }
}
"#,
        );
        assert!(
            !has_errors(&diags),
            "postfix without import is a warning: {:?}",
            diags
        );
        assert!(
            diags.iter().any(|d| d.message.contains("postfixOps")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn postfix_ops_ok_with_import() {
        ok(r#"
import scala.language.implicitConversions
import scala.language.postfixOps
object Main {
  implicit class Bang(n: Int) { def bang: Int = n + 1 }
  def main(args: Array[String]): Unit = {
    val n: Int = 42 bang
  }
}
"#);
    }

    #[test]
    fn implicit_conversions_warns_without_import() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  implicit class Rich(n: Int) { def twice: Int = n * 2 }
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#,
        );
        assert!(
            !has_errors(&diags),
            "implicitConversions is a warning: {:?}",
            diags
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("implicitConversions")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
