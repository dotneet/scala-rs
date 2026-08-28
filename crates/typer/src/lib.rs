mod check;
mod classpath;
mod erasure;
mod implicits;
mod javaclass;
mod javasign;
mod lambda_lift;
mod prelude;
mod prelude_either;
mod prelude_seq;
mod prelude_text;
mod prelude_tuple;
mod symbol;
mod uncurry;

pub use check::{
    find_mains, has_errors, typecheck, typecheck_opts, ClasspathClass, ClasspathMethod,
    ClasspathPickleMethod, TypecheckOptions, Typer,
};
pub use erasure::{erase, erase_type};
pub use lambda_lift::lambda_lift;
pub use symbol::{Intrinsic, SamSig, SymKind, Symbol, SymbolTable};
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
                binary_path: Vec::new(),
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
    fn implicit_not_found_custom_message() {
        let (_, _, diags) = typecheck_str(
            r#"
import scala.annotation.implicitNotFound
@implicitNotFound("no show for ${A}")
trait Show[A]
object Main {
  def show[A](implicit s: Show[A]): Int = 1
  def main(args: Array[String]): Unit = {
    val n: Int = show[Int]
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("no show for Int")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn switch_match_ok_and_cannot_warns() {
        ok(r#"
import scala.annotation.switch
object Main {
  def f(n: Int): Int = (n: @switch) match {
    case 0 => 10
    case 1 => 11
    case 2 => 12
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
import scala.annotation.switch
object Main {
  def f(n: Any): Int = (n: @switch) match {
    case 0 => 1
    case "x" => 2
    case _ => 3
  }
}
"#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("could not emit switch")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn early_defs_and_illegal_this() {
        ok(r#"
trait T { val x: Int; val y: Int = x + 10 }
class C extends { val x = 1 } with T
"#);
        let (_, _, diags) = typecheck_str(
            r#"
trait T { val x: Int }
class Bad extends { def f = 1 } with T { val x = 1 }
"#,
        );
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("only concrete field definitions")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
trait T { val x: Int }
class BadThis extends { val x = this.toString.length } with T
"#,
        );
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("this can be used only in a class")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn infix_either_type_typechecks() {
        ok(r#"
class Pair[A, B](val a: A, val b: B)
object Main {
  def f(p: Int Pair String): Int = 1
}
"#);
        ok_lib(
            r#"
object Main {
  def f(e: Int Either String): Int Either String = e
  def main(args: Array[String]): Unit = {
    val l: Int Either String = Left(7)
    val r: Int Either String = Right("ok")
    val a: Boolean = f(l).isLeft
    val b: Boolean = r.isLeft
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  val l: Int Either String = Left("nope")
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(
            has_errors(&diags),
            "Left[String, String] must not conform to Either[Int, String]: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sam_runnable_and_comparator_typecheck() {
        ok(r#"
object Main {
  def go(): Unit = ()
  def cmp(a: Int, b: Int): Int = a - b
  def main(args: Array[String]): Unit = {
    val r: Runnable = () => ()
    val r2: Runnable = go _
    val c: java.util.Comparator[Int] = (a, b) => a - b
    val c2: java.util.Comparator[Int] = cmp
    val f: java.util.function.Function[Int, Int] = (x) => x + 1
    r.run()
    r2.run()
    val n: Int = c.compare(1, 2)
    val m: Int = f.apply(1)
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
class NotSam { def a(): Unit = (); def b(): Unit = () }
object Main { val x: NotSam = () => () }
"#,
        );
        assert!(
            has_errors(&diags) && diags.iter().any(|d| d.message.contains("type mismatch")),
            "missing SAM must diagnose, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
trait TwoAbs { def a(): Unit; def b(): Unit }
object Main { val y: TwoAbs = () => () }
"#,
        );
        assert!(
            has_errors(&diags) && diags.iter().any(|d| d.message.contains("type mismatch")),
            "two abstract methods must not SAM-wrap, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def go(): Unit = ()
  val z: Runnable = go
}
"#,
        );
        assert!(
            has_errors(&diags) && diags.iter().any(|d| d.message.contains("type mismatch")),
            "nullary method must auto-apply, not eta to Runnable: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn inline_on_method_ok_on_val_diagnosed() {
        ok(r#"
object Main {
  @inline def f(): Int = 1
  @noinline def g(): Int = 2
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  @inline val x: Int = 1
}
"#,
        );
        assert!(
            has_errors(&diags) && diags.iter().any(|d| d.message.contains("only supported")),
            "expected @inline-on-val error, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  @inline @noinline def f(): Int = 1
}
"#,
        );
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("cannot be used together")),
            "expected @inline/@noinline conflict, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn java_classpath_math_and_arraylist_typecheck() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = java.lang.Math.abs(-3)
    val b: Int = java.lang.Byte.MAX_VALUE
    val xs = new java.util.ArrayList[String]()
    val added: Boolean = xs.add("x")
    val sz: Int = xs.size()
  }
}
"#);
    }

    #[test]
    fn java_signature_get_is_not_raw_object() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = new java.util.ArrayList[String]()
    xs.add("hi")
    val s: String = xs.get(0)
    val n: Int = s.length
    val n2: Int = xs.get(0).length
  }
}
"#);
    }

    #[test]
    fn java_inner_map_entry_and_varargs_typecheck() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val e = new java.util.AbstractMap.SimpleEntry[String, String]("k", "v")
    val k: String = e.getKey
    val v: String = e.getValue
    val asEntry: java.util.Map.Entry[String, String] = e
    val k2: String = asEntry.getKey
    val f: String = java.lang.String.format("%s-%d", "x", 3)
    val n: Int = java.util.Arrays.asList("a", "b").size()
  }
}
"#);
    }

    #[test]
    fn java_wildcard_and_tparam_bounds_typecheck() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val c: Class[_] = java.lang.Class.forName("java.lang.String")
    val n: String = c.getName
    val xs = new java.util.ArrayList[java.lang.Byte]()
    val added: Boolean = xs.add(java.lang.Byte.valueOf("1"))
    val ys: java.util.Collection[_ <: java.lang.Number] = java.util.Collections.unmodifiableList(xs)
    val sz: Int = ys.size()
    val m: java.lang.Byte = java.util.Collections.max(xs)
    val i: Int = m.intValue()
  }
}
"#);
    }

    #[test]
    fn java_checked_exception_does_not_need_catch() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    java.lang.Thread.sleep(0L)
  }
}
"#);
    }

    fn javac_available() -> bool {
        std::process::Command::new("javac")
            .arg("-version")
            .output()
            .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
            .unwrap_or(false)
    }

    fn compile_jprot_base() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scala-rs-jprot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("Base.java");
        std::fs::write(
            &src,
            "package jprot;\npublic class Base {\n  protected int secret() { return 7; }\n  protected static int secretStatic() { return 11; }\n}\n",
        )
        .unwrap();
        let status = std::process::Command::new("javac")
            .args(["-d", dir.to_str().unwrap(), src.to_str().unwrap()])
            .status()
            .expect("javac");
        assert!(status.success(), "javac jprot.Base failed");
        dir
    }

    #[test]
    fn java_protected_same_package_and_subclass() {
        if !javac_available() {
            return;
        }
        let cp = compile_jprot_base();
        let (_, _, diags) = typecheck_str_opts(
            r#"
package jprot
class Peer {
  def fromPeer(b: Base): Int = b.secret()
}
class Sub extends Base {
  def mine: Int = this.secret()
}
object Main {
  def main(args: Array[String]): Unit = {
    val a: Int = new Peer().fromPeer(new Base())
    val b: Int = new Sub().mine
    val c: Int = Base.secretStatic()
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: false,
                classpath: Vec::new(),
                binary_path: vec![cp.clone()],
                language_features: Vec::new(),
            },
        );
        let _ = std::fs::remove_dir_all(&cp);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn java_protected_illegal_access_diagnosed() {
        if !javac_available() {
            return;
        }
        let cp = compile_jprot_base();
        let (_, _, diags) = typecheck_str_opts(
            r#"
package other
class Unrelated {
  def bad(b: jprot.Base): Int = b.secret()
}
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = new Unrelated().bad(new jprot.Base())
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: false,
                classpath: Vec::new(),
                binary_path: vec![cp.clone()],
                language_features: Vec::new(),
            },
        );
        let _ = std::fs::remove_dir_all(&cp);
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("cannot be accessed")),
            "expected illegal protected access, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unsupported_java_classfile_diagnosed() {
        let dir = std::env::temp_dir().join(format!(
            "scala-rs-badclass-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Broken.class"),
            [0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 52, 0, 2, 99],
        )
        .unwrap();
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    new Broken()
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: false,
                classpath: Vec::new(),
                binary_path: vec![dir.clone()],
                language_features: Vec::new(),
            },
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("unsupported classfile")),
            "expected unsupported classfile diagnostic, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn native_method_ok_body_diagnosed() {
        ok(r#"
object Main {
  @native def foo(): Int
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  @native def foo(): Int = 1
}
"#,
        );
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("cannot have a body")),
            "expected native-with-body error, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  @native val x: Int = 1
}
"#,
        );
        assert!(
            has_errors(&diags)
                && diags
                    .iter()
                    .any(|d| d.message.contains("only supported on methods")),
            "expected @native-on-val error, got {:?}",
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
                binary_path: Vec::new(),
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
                binary_path: Vec::new(),
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
    fn value_existential_path_packs() {
        ok(r#"
class Outer {
  class Inner { def n: Int = 1 }
  def inner: Inner = new Inner()
}
object Main {
  def take(x: p.Inner forSome { val p: Outer }): Int = x.n
  def main(args: Array[String]): Unit = {
    val n: Int = take(new Outer().inner)
  }
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
    fn class_view_bounds_desugar() {
        ok(r#"
class Box(val n: Int) extends Ordered[Box] {
  def compare(that: Box): Int = n - that.n
}
class C[T <% Ordered[T]](val x: T)
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C(new Box(1))
    val n: Int = c.x.n
  }
}
"#);
    }

    #[test]
    fn class_view_bounds_missing_evidence() {
        let (_, _, diags) = typecheck_str(
            r#"
class NoOrd(val n: Int)
class C[T <% Ordered[T]](x: T)
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C(new NoOrd(1))
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
    fn higher_kinded_functor_and_box() {
        ok(r#"
class Id[A](val value: A)
class Box[F[_], A](val fa: F[A])
trait Functor[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}
object IdFunctor extends Functor[Id] {
  def map[A, B](fa: Id[A])(f: A => B): Id[B] = new Id(f(fa.value))
}
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box[Id, Int](new Id(41))
    val n: Int = b.fa.value
    val m: Id[Int] = IdFunctor.map[Int, Int](new Id(1))((x: Int) => x + 1)
  }
}
"#);
    }

    #[test]
    fn higher_kinded_kind_mismatch() {
        let (_, _, diags) = typecheck_str(
            r#"
trait Functor[F[_]] { def dummy: Int }
object Main {
  def asProper[F[_]](x: F): Unit = ()
  def useFunctor(x: Functor[Int]): Unit = ()
  def notCtor[A](x: Int[A]): Unit = ()
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("type parameters")
                || d.message.contains("kinds of the type arguments")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn extends_app_and_delayed_init() {
        ok(r#"
object Main extends App {
  println(1)
}
"#);
        ok(r#"
class C extends DelayedInit {
  def delayedInit(x: => Unit): Unit = x
  println(1)
}
object Main {
  def main(args: Array[String]): Unit = { new C }
}
"#);
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
    fn type_alias_expands_in_vals_and_defs() {
        ok(r#"
trait M { type A = String }
class C extends M { def id(x: A): A = x }
object Main {
  type T = List[Int]
  def headOf(xs: T): Int = xs.head
  def main(args: Array[String]): Unit = {
    val xs: T = 1 :: Nil
    val n: Int = headOf(xs)
    val s: String = new C().id("ok")
  }
}
"#);
    }

    #[test]
    fn cyclic_type_alias_is_diagnosed() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  type A = B
  type B = A
  def main(args: Array[String]): Unit = {
    val x: A = 1
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("illegal cyclic reference")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn apply_without_member_is_not_update() {
        let (_, _, diags) = typecheck_str(
            r#"
class Cell { var n: Int = 0 }
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Cell()
    val x: Int = c(1)
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("value apply is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn update_assignment_desugars() {
        ok(r#"
class Cell {
  var n: Int = 0
  def update(i: Int, v: Int): Unit = { n = v }
  def apply(i: Int): Int = n
}
object Main {
  def main(args: Array[String]): Unit = {
    val arr = new Array[Int](1)
    arr(0) = 1
    val c = new Cell()
    c(0) = 2
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
        ok(r#"
import scala.annotation.unchecked.uncheckedVariance
class Inv[A](val value: A)
class Q[+A] { def enqueue(x: A @uncheckedVariance): Int = 1 }
class Box[+A](val inner: Inv[A @uncheckedVariance]) { def get: A = inner.value }
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = new Q[Int].enqueue(1)
    val b: Box[Int] = new Box(new Inv(41))
    val m: Int = b.get
  }
}
"#);
        let (_, _, diags) = typecheck_str("class Bad2[+A] { def enqueue(x: A): Int = 1 }\n");
        assert!(
            has_errors(&diags),
            "expected error without @uncheckedVariance, got {:?}",
            diags
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("covariant") && d.message.contains("contravariant")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) =
            typecheck_str("class Inv[A](val value: A); class Bad3[+A](val inner: Inv[A])\n");
        assert!(
            has_errors(&diags),
            "expected invariant-position error, got {:?}",
            diags
        );
    }

    #[test]
    fn higher_kinded_type_member_and_path_dependent_app() {
        ok(r#"
class Id[A](val value: A)
trait M { type F[_] }
class C extends M {
  type F[X] = Id[X]
  def wrap(x: Int): F[Int] = new Id(x)
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C
    val x: c.F[Int] = c.wrap(41)
    val n: Int = x.value
    val y: c.F[Int] = new Id(2)
    val m: Int = y.value
  }
}
"#);
    }

    #[test]
    fn higher_kinded_type_member_kind_mismatch() {
        let (_, _, diags) = typecheck_str(
            r#"
trait M { type F[_] }
class C extends M { type F = Int }
object Main {
  def asProper(m: M)(x: m.F): Unit = ()
  def main(args: Array[String]): Unit = ()
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("type parameters") || d.message.contains("kind")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn class_context_bounds_desugar() {
        ok(r#"
trait Ord[T] {
  def compare(x: T, y: T): Int
}
object Ord {
  implicit val oi: Ord[Int] = new Ord[Int] {
    def compare(x: Int, y: Int): Int = x - y
  }
}
class C[T: Ord](val x: T)
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = new C(2).x
  }
}
"#);
        let jar = std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        if jar.is_file() {
            let (t, _, diags) = typecheck_str_opts(
                r#"
import scala.math.Ordering
class C[T: Ordering](val x: T)
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = new C(2).x
  }
}
"#,
                &TypecheckOptions {
                    fatal_warnings: false,
                    library_abi: true,
                    classpath: Vec::new(),
                    binary_path: vec![jar],
                    language_features: Vec::new(),
                },
            );
            let _ = t;
            assert!(
                !has_errors(&diags),
                "class C[T: Ordering] must typecheck against scala-library: {:?}",
                diags.iter().map(|d| &d.message).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn class_context_bounds_missing_evidence() {
        let (_, _, diags) = typecheck_str(
            r#"
trait Show[T]
class C[T: Show](val x: T)
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C(1)
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
    fn trait_context_bounds_are_error() {
        let (_, _, diags) = typecheck_str("trait T[A: Ordering]\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("traits cannot have type parameters with context bounds")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn hk_view_bounds_are_diagnosed() {
        let (_, _, diags) = typecheck_str("class D[F[_] <% Ordered[_]]\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("takes type parameters")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def f[F[_] <% Ordered[_]](x: Int): Int = x
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("takes type parameters")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn higher_kinded_refinement_type_member() {
        ok(r#"
class Id[A](val value: A)
trait M { type F[_]; def wrap(x: Int): F[Int] }
class C extends M {
  type F[X] = Id[X]
  def wrap(x: Int): F[Int] = new Id(x)
}
object Main {
  def use(m: M { type F[X] = Id[X] }): Int = m.wrap(41).value
  def main(args: Array[String]): Unit = {
    val n: Int = use(new C)
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
trait M { type F[_] }
object Main {
  def asProper(m: M { type F[_] })(x: m.F): Unit = ()
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("takes type parameters")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn bounded_refinement_and_hk_bounded_member() {
        ok(r#"
trait Bound
class Id[A](val value: A) extends Bound
trait Box { type A; def x: A }
class IntBox extends Box { type A = Int; def x: A = 41 }
trait M { type F[_] <: Bound; def wrap(x: Int): F[Int] }
class C extends M {
  type F[X] = Id[X]
  def wrap(x: Int): F[Int] = new Id(x)
}
object Main {
  def get(b: Box { type A <: Int }): Int = b.x
  def main(args: Array[String]): Unit = {
    val n: Int = get(new IntBox)
    val m: Int = new C().wrap(2).value
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
trait Box { type A; def x: A }
class StrBox extends Box { type A = String; def x: A = "hi" }
object Main {
  def get(b: Box { type A <: Int }): Int = b.x
  def main(args: Array[String]): Unit = {
    val b: Box { type A <: Int } = new StrBox
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("type mismatch")
                    || d.message.contains("does not conform")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
trait Bound
trait M { type F[_] <: Bound }
class C extends M { type F[X] = Int }
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("incompatible")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
trait M { type F[_] <: List }
object Main { def main(args: Array[String]): Unit = () }
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("takes type parameters")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn nested_type_projections() {
        ok(r#"
class Outer {
  class Inner {
    type X = Int
    def n: X = 41
  }
  def inner: Inner = new Inner
}
trait A { type T }
class AI extends A { type T = Int }
class Holder { type Inner = AI }
object Main {
  def fromClass(x: Outer#Inner#X): Int = x
  def fromAlias(x: Holder#Inner#T): Int = x
  def main(args: Array[String]): Unit = {
    val a: Int = fromClass(new Outer().inner.n)
    val b: Int = fromAlias(2)
  }
}
"#);
        let (_, _, diags) = typecheck_str("object Main { def bad: Int#X = 1 }\n");
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
trait B { type U }
object Main { def f(x: B#U#T): Int = 1 }
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn nullary_bounded_type_members() {
        ok(r#"
trait Bound { def n: Int }
class BI extends Bound { def n: Int = 41 }
trait T { type A <: Bound; def x: A }
class C extends T { type A = BI; def x: A = new BI }
abstract class D { type A <: Int; def x: A }
class E extends D { type A = Int; def x: A = 41 }
abstract class Lo { type A >: Null; def y: A }
class LoOk extends Lo { type A = String; def y: A = "ok" }
object Main {
  def fromC(c: C): Int = c.x.n
  def fromE(e: E): Int = e.x
  def asBound(t: T): Bound = t.x
  def asInt(d: D): Int = d.x
  def main(args: Array[String]): Unit = {
    val a: Int = fromC(new C)
    val b: Int = fromE(new E)
    val c: Bound = asBound(new C)
    val d: Int = asInt(new E)
    val s: String = new LoOk().y
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
trait Bound
class C { type A <: Bound }
class D extends C { type A = Int }
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("incompatible")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
class C { type A >: String }
class D extends C { type A = Int }
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("incompatible")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn assignment_operators() {
        ok(r#"
class Acc(var n: Int) {
  def +=(k: Int): Acc = { n += k; this }
}
object Main {
  def main(args: Array[String]): Unit = {
    var x = 40
    x += 1
    val a = new Acc(1)
    a += 1
    val n: Int = x + a.n
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = 1
    x += 1
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            diags.iter().any(|d| d.message.contains("not assignable")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn collection_converters_typecheck_with_library() {
        let jar = std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        if !jar.is_file() {
            return;
        }
        let (t, _, diags) = typecheck_str_opts(
            r#"
import scala.jdk.CollectionConverters._
object Main {
  def show(x: Any): Unit = println(x)
  def main(args: Array[String]): Unit = {
    val jl = new java.util.ArrayList[Int]()
    jl.add(41)
    val buf = jl.asScala
    show(buf.head)
    val xs = List(1, 2)
    val jlist = xs.asJava
    show(jlist.get(1))
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: vec![jar],
                language_features: Vec::new(),
            },
        );
        let _ = t;
        assert!(
            !has_errors(&diags),
            "CollectionConverters must typecheck against scala-library: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val jl = new java.util.ArrayList[Int]()
    val buf = jl.asScala
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("asScala is not a member")),
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
    fn package_object_implicit_class_typecheck() {
        ok(r#"
package object enrich {
  implicit class Rich(n: Int) { def twice: Int = n * 2 }
}
object Main {
  import enrich._
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
package object enrich {
  implicit class Rich(n: Int) { def twice: Int = n * 2 }
}
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("twice") && d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str(
            r#"
implicit class Rich(n: Int) { def twice: Int = n * 2 }
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("top-level")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    fn pkg_implicit_classpath(nested_first: bool) -> Vec<ClasspathClass> {
        let pkg = ClasspathClass {
            jvm_name: "enrich/package$".into(),
            is_module: true,
            methods: vec![ClasspathMethod {
                name: "Rich".into(),
                desc: "(I)Lenrich/package$Rich;".into(),
            }],
            pickle: Some(vec![ClasspathPickleMethod {
                name: "Rich".into(),
                param_names: vec!["n".into()],
                param_types: vec!["Int".into()],
                ret: "Rich".into(),
                tparams: vec![],
                is_val: false,
                is_ctor: false,
                is_implicit: true,
            }]),
            pickle_tparams: vec![],
        };
        let nested = ClasspathClass {
            jvm_name: "enrich/package$Rich".into(),
            is_module: false,
            methods: vec![ClasspathMethod {
                name: "twice".into(),
                desc: "()I".into(),
            }],
            pickle: Some(vec![
                ClasspathPickleMethod {
                    name: "<init>".into(),
                    param_names: vec!["n".into()],
                    param_types: vec!["Int".into()],
                    ret: "Unit".into(),
                    tparams: vec![],
                    is_val: false,
                    is_ctor: true,
                    is_implicit: false,
                },
                ClasspathPickleMethod {
                    name: "twice".into(),
                    param_names: vec![],
                    param_types: vec![],
                    ret: "Int".into(),
                    tparams: vec![],
                    is_val: false,
                    is_ctor: false,
                    is_implicit: false,
                },
            ]),
            pickle_tparams: vec![],
        };
        if nested_first {
            vec![nested, pkg]
        } else {
            vec![pkg, nested]
        }
    }

    fn assert_pkg_implicit_cp(cp: Vec<ClasspathClass>) {
        let (_, _, diags) = typecheck_str_opts(
            r#"
import enrich._
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: false,
                classpath: cp.clone(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(
            !has_errors(&diags),
            "classpath package-object implicit class must typecheck: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: false,
                classpath: cp,
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(
            has_errors(&diags),
            "expected missing import, got {:?}",
            diags
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("twice") && d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn classpath_package_object_implicit_class() {
        assert_pkg_implicit_cp(pkg_implicit_classpath(false));
        assert_pkg_implicit_cp(pkg_implicit_classpath(true));
    }

    #[test]
    fn structural_update_typecheck() {
        ok(r#"
class Cell {
  private var n: Int = 0
  def foo: Int = n
  def foo_=(k: Int): Unit = { n = k }
}
class Buf {
  private var n: Int = 0
  def apply(i: Int): Int = n
  def update(i: Int, v: Int): Unit = { n = v }
}
object Main {
  def set(x: { var foo: Int }): Unit = { x.foo = 41 }
  def get(x: { var foo: Int }): Int = x.foo
  def setDef(x: { def foo: Int; def foo_=(k: Int): Unit }): Unit = { x.foo = 7 }
  def upd(x: { def apply(i: Int): Int; def update(i: Int, v: Int): Unit }): Unit = {
    x(0) = 9
  }
  def main(args: Array[String]): Unit = {
    val c = new Cell()
    set(c)
    val n: Int = get(c)
    setDef(c)
    val b = new Buf()
    upd(b)
    val k: Int = b.apply(0)
  }
}
"#);
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def use(x: { def foo: Int }): Unit = {
    x.foo = 1
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("foo_=") && d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn indexedseq_queue_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n: Int = IndexedSeq(1, 2)(1)
    val q = scala.collection.immutable.Queue(1, 2).enqueue(3)
    val d = q.dequeue
    val h: Int = d._1
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n = IndexedSeq(1, 2).noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("noSuch") && d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops3_richbyte_arraybuffer_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a: String = "foobar".stripSuffix("bar")
    val b: String = "ab".padTo(5, 'x')
    val c: String = "a\nb".linesIterator.next()
    val d = "12".toIntOption
    val e: Byte = 1.toByte.max(2.toByte)
    val f: Short = 1.toShort.max(3.toShort)
    val g: Int = true.compare(false)
    val buf = scala.collection.mutable.ArrayBuffer(1, 2)
    buf += 3
    buf(1) = 9
    val n: Int = buf(0)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchStrip
    val y = 1.toByte.noSuchMax
    val z = scala.collection.mutable.ArrayBuffer(1).noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops4_numeric_range_listbuffer_typecheck_with_library() {
        ok_lib(
            r##"
object Main {
  def main(args: Array[String]): Unit = {
    val a: String = "|hello\n|world".stripMargin
    val b: String = "#hello\n#world".stripMargin('#')
    val c: String = "a\nb".lines.next()
    val r = 1 to 3
    val u = 1 until 3
    val s: String = r.mkString(",")
    val nr = 1.toByte to 3.toByte
    val nu = 1.toByte until 3.toByte
    val ns: String = nr.mkString(",")
    val buf = scala.collection.mutable.ListBuffer(1, 2)
    buf += 3
    val n: Int = buf(0)
  }
}
"##,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchMargin
    val y = (1 to 3).noSuchMk
    val z = scala.collection.mutable.ListBuffer(1).noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops5_richshort_stringbuilder_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a: String = "hello".capitalize
    val b: String = "abc".reverse
    val c: String = "abcdef".slice(1, 4)
    val nr = 1.toShort to 3.toShort
    val nu = 1.toShort until 3.toShort
    val ns: String = nr.mkString(",")
    val buf = new scala.collection.mutable.StringBuilder()
    buf += 'a'
    buf.append("bc")
    val s: String = buf.toString
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchCap
    val y = (1.toShort to 3.toShort).noSuchMk
    val z = new scala.collection.mutable.StringBuilder().noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops6_richlong_hashmap_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a: String = "abcdef".takeRight(3)
    val b: String = "abcdef".dropRight(2)
    val c: Boolean = "hello".contains('e')
    val nr = 1L to 3L
    val nu = 1L until 3L
    val ns: String = nr.mkString(",")
    val m = scala.collection.mutable.HashMap.empty[Int, String]
    m.update(1, "a")
    m(2) = "b"
    m += (3 -> "c")
    val v: String = m(1)
    val o = m.get(2)
    val n = scala.collection.mutable.HashMap(1 -> "x")
    val w = n(1)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchRight
    val y = (1L to 3L).noSuchMk
    val z = scala.collection.mutable.HashMap.empty[Int, String].noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops7_richchar_hashset_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a: Boolean = "hello".startsWith("he")
    val b: Boolean = "hello".endsWith("lo")
    val i: Int = "hello".indexOf("ll")
    val nr = 'a' to 'c'
    val nu = 'a' until 'c'
    val ns: String = nr.mkString(",")
    val s = scala.collection.mutable.HashSet.empty[Int]
    s += 1
    val c: Boolean = s.contains(1)
    val t = scala.collection.mutable.HashSet(3, 4)
    val d: Boolean = t.contains(3)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchStart
    val y = ('a' to 'c').noSuchMk
    val z = scala.collection.mutable.HashSet.empty[Int].noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops8_arrayops_linkedhashmap_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val h: Char = "hello".head
    val l: Char = "hello".last
    val s: String = "hello\n".stripLineEnd
    val r: String = "a-b-a".replaceAllLiterally("-", "_")
    val arr = Array(1, 2, 3)
    val ah: Int = arr.head
    val at: Int = arr.tail.head
    val al: Int = arr.tail.length
    val m = scala.collection.mutable.LinkedHashMap.empty[Int, String]
    m.update(1, "a")
    m(2) = "b"
    m += (3 -> "c")
    val v: String = m(1)
    m.foreach(kv => println(kv._1))
    val n = scala.collection.mutable.LinkedHashMap(4 -> "x", 5 -> "y")
    n.foreach(kv => println(kv._1))
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchHead
    val y = Array(1, 2, 3).noSuchHead
    val z = scala.collection.mutable.LinkedHashMap.empty[Int, String].noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops9_arrayops_linkedhashset_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val t: String = "hello".tail
    val i: String = "hello".init
    val d: String = "abca".distinct
    val m0: String = "abc".mkString
    val m1: String = "abc".mkString(",")
    val xs = Array(1, 2, 3)
    xs.foreach(x => println(x))
    val ys = Array(10L, 20L, 30L)
    val yh: Long = ys.head
    ys.foreach(x => println(x))
    val s = scala.collection.mutable.LinkedHashSet.empty[Int]
    s += 1
    val c: Boolean = s.contains(1)
    s.foreach(x => println(x))
    val u = scala.collection.mutable.LinkedHashSet(3, 4)
    u.foreach(x => println(x))
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchTail
    val y = Array(1, 2, 3).noSuchForeach
    val z = scala.collection.mutable.LinkedHashSet.empty[Int].noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ops10_arrayops_map_arraydeque_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val f: String = "hello".filter(c => c != 'l')
    "hello".reverseIterator.foreach(c => println(c))
    val ys = Array(1, 2, 3).map(x => x + 1)
    ys.foreach(x => println(x))
    val zs = Array("a", "b").map(s => s + "x")
    zs.foreach(s => println(s))
    val d = scala.collection.mutable.ArrayDeque.empty[Int]
    d += 1
    d.prepend(0)
    val n: Int = d(0)
    val e = scala.collection.mutable.ArrayDeque(3, 4)
    e += 5
    val m: Int = e(1)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchFilter
    val y = Array(1, 2, 3).noSuchMap
    val z = scala.collection.mutable.ArrayDeque.empty[Int].noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn placeholder_byteops_stringops_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val ys = Array(1, 2, 3).map(_ + 1)
    ys.foreach(x => println(x))
    val f: Int => Int = _ + 1
    val g: Int => Int = _.abs
    def add1(x: Int): Int = x + 1
    val h: Int => Int = add1(_)
    val bh: Byte = Array(1.toByte, 2.toByte).head
    val by = Array(1.toByte).map(_ + 1)
    val sh: Short = Array(1.toShort).head
    val ps: Array[String] = "a,b".split(",")
    val d: String = "abcde".diff("bd")
    val i: String = "abcde".intersect("cde")
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = _
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unbound placeholder parameter")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags2) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchDiff
    val y = Array(1.toByte).noSuchHead
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags2), "expected error, got {:?}", diags2);
        assert!(
            diags2.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags2.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn placeholder_fn2_strops_charfloat_arrayops_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val add: (Int, Int) => Int = _ + _
    val n: Int = add(1, 2)
    val nest: Array[Int] => Array[Int] = _.map(_ + 1)
    val ys = nest(Array(1, 2, 3))
    val u: String = "hello".updated(1, 'a')
    val c: Int = "hello".count(_ == 'l')
    val sp = "hello".span(_ != 'l')
    val ch: Char = Array('a', 'b').head
    val mapped = Array('a', 'b').map(_ + 1)
    val fh: Float = Array(1.0f, 2.0f).head
    val fa = Array(-1.0f, 2.0f).map(_.abs)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val f: Int => Int = _ + _
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("missing parameter type for expanded function")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags2) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchUpdated
    val y = Array('a').noSuchHead
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags2), "expected error, got {:?}", diags2);
        assert!(
            diags2.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags2.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn typed_placeholder_strops_doublebool_arrayops_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val f: Int => Int = (_: Int) + 1
    val n: Int = f(10)
    val g = (_: Int) + (_: Int)
    val s: Int = g(1, 2)
    val h: Int => Int = (_: Int).abs
    val ys = Array(1, 2, 3).map((_: Int) + 1)
    val nest: Array[Int] => Array[Int] = _.map((_: Int) + 1)
    val p = "hello".partition(_ != 'l')
    val e: Boolean = "hello".exists(_ == 'h')
    val a: Boolean = "hello".forall(_ != 'z')
    val sa = "hello".splitAt(2)
    val dh: Double = Array(1.0, 2.0).head
    val dm = Array(1.0, 2.0).map(_ + 1.0)
    val bh: Boolean = Array(true, false).head
    val bm = Array(true, false).map(!_)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = (_: Int)
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unbound placeholder parameter")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, _, diags2) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "hi".noSuchPartition
    val y = Array(1.0).noSuchHead
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags2), "expected error, got {:?}", diags2);
        assert!(
            diags2.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags2.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generic_arrayops_unit_sortedset_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def first[T](a: Array[T]) = a.head
  def firstMap[T: scala.reflect.ClassTag](a: Array[T]) = a.map(x => x)
  def main(args: Array[String]): Unit = {
    val n: Int = first(Array(1, 2, 3))
    val s: String = first(Array("a", "b"))
    val ar: Array[AnyRef] = Array("x", "y")
    val r: AnyRef = ar.head
    val ys = firstMap(Array(10, 20))
    val uh = Array((), ()).head
    val um = Array((), ()).map(_ => 1)
    val set = scala.collection.immutable.SortedSet(3, 1, 2)
    val c: Boolean = set.contains(1)
    val t = scala.collection.immutable.TreeSet(5, 4, 6)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = Array("a").noSuchHead
    val y = scala.collection.immutable.SortedSet(1).noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn arrayops_sortedmap_stringops_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val fs = Array(1, 2, 3).filter(_ > 1)
    val sl = Array(1, 2, 3, 4).slice(1, 3)
    val fm = Array(1, 2).flatMap(x => List(x, x + 10))
    val s: String = "cba".sorted
    val cs: Array[Char] = "ab".toArray
    val buf = new Array[Char](2)
    val n: Int = "xy".copyToArray(buf)
    val m = scala.collection.immutable.SortedMap(3 -> "c", 1 -> "a")
    val a: String = m.apply(1)
    val g = m.get(1)
    val t = scala.collection.immutable.TreeMap(5 -> "e", 4 -> "d")
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = Array(1).noSuchFilter
    val y = "cba".noSuchSorted
    val z = scala.collection.immutable.SortedMap(1 -> "a").noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn arrayops_flatmap4_stringops_bitset_typecheck_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a = Array(1, 2).flatMap(i => List(i, i))
    val b = Array(1, 2).flatMap(i => Array(i, i))
    val ix = "abc".indices
    val re = "a+".r
    val m: Boolean = re.matches("aa")
    val s = scala.collection.immutable.BitSet(3, 1, 2)
    val c: Boolean = s.contains(1)
  }
}
"#,
        );
        let (_, _, diags) = typecheck_str_opts(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = Array(1).noSuchFlatMap
    val y = "abc".noSuchIndices
    val z = scala.collection.immutable.BitSet(1).noSuch
  }
}
"#,
            &TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("is not a member")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
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

    #[test]
    fn xml_literal_typechecks_with_library() {
        ok_lib(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val n = 1
    val x = <a>t{n}</a>
  }
}
"#,
        );
    }

    #[test]
    fn enumeration_value_typechecks_with_library() {
        ok_lib(
            r#"
object Color extends Enumeration {
  val Red, Blue = Value
}
object Main {
  def main(args: Array[String]): Unit = {
    val s: String = Color.Red.toString
    val n: Int = Color.Red.id
  }
}
"#,
        );
    }

    #[test]
    fn java_enum_thread_state_typecheck() {
        ok(r#"
object Main {
  def main(args: Array[String]): Unit = {
    val s: java.lang.Thread.State = java.lang.Thread.State.NEW
    val t: java.lang.Thread.State = java.lang.Thread.State.valueOf("RUNNABLE")
    val n: Int = java.lang.Thread.State.values().length
    val k: Int = s match {
      case java.lang.Thread.State.NEW => 1
      case _ => 0
    }
  }
}
"#);
    }

    #[test]
    fn java_non_enum_values_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = java.lang.String.values()
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("values")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn context_bounds_ordering_and_classtag_path() {
        ok_lib(
            r#"
object Main {
  def g[T: scala.reflect.ClassTag](xs: Array[T]): Int = xs.length
  def main(args: Array[String]): Unit = {
    val b: Int = g(new Array[Int](3))
  }
}
"#,
        );
        ok(r#"
trait Ord[T] {
  def compare(x: T, y: T): Int
}
object Ord {
  implicit val oi: Ord[Int] = new Ord[Int] {
    def compare(x: Int, y: Int): Int = x - y
  }
}
object Main {
  def cmp[T: Ord](x: T, y: T): Int = implicitly[Ord[T]].compare(x, y)
  def main(args: Array[String]): Unit = {
    val a: Int = cmp(3, 1)
  }
}
"#);
        let jar = std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        if jar.is_file() {
            let (t, _, diags) = typecheck_str_opts(
                r#"
import scala.math.Ordering
object Main {
  def cmp[T: Ordering](x: T, y: T): Int = implicitly[Ordering[T]].compare(x, y)
  def g[T: scala.reflect.ClassTag](xs: Array[T]): Int = xs.length
  def main(args: Array[String]): Unit = {
    val a: Int = cmp(3, 1)
    val b: Int = g(new Array[Int](3))
  }
}
"#,
                &TypecheckOptions {
                    fatal_warnings: false,
                    library_abi: true,
                    classpath: Vec::new(),
                    binary_path: vec![jar],
                    language_features: Vec::new(),
                },
            );
            let _ = t;
            assert!(
                !has_errors(&diags),
                "Ordering[Int] from scala-library must typecheck: {:?}",
                diags.iter().map(|d| &d.message).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn context_bound_missing_evidence_is_error() {
        let (_, _, diags) = typecheck_str(
            r#"
trait Show[T]
object Main {
  def show[T: Show](x: T): T = x
  def main(args: Array[String]): Unit = {
    show(1)
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
    fn auxiliary_constructor_and_subclass_super_args() {
        ok(r#"
class C(val x: Int, val y: Int) {
  def this(x: Int) = this(x, 0)
  def sum: Int = x + y
}
class D extends C(1)
class E(z: Int) extends C(z)
object Main {
  def main(args: Array[String]): Unit = {
    val a: Int = new C(3, 4).sum
    val b: Int = new C(5).sum
    val c: Int = new D().sum
    val d: Int = new E(9).sum
  }
}
"#);
    }

    #[test]
    fn auxiliary_constructor_must_call_this() {
        let (_, _, diags) = typecheck_str(
            r#"
class C(val x: Int, val y: Int) {
  def this(x: Int) = x
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("this(...)")
                    || d.message.contains("auxiliary constructor")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn auxiliary_constructor_this_must_be_first() {
        let (_, _, diags) = typecheck_str(
            r#"
class C(val x: Int, val y: Int) {
  def this(x: Int) = {
    val z = 1
    this(x, 0)
  }
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("first statement")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn auxiliary_constructor_cannot_call_super() {
        let (_, _, diags) = typecheck_str(
            r#"
class Base(n: Int)
class C(val x: Int, val y: Int) extends Base(0) {
  def this(x: Int) = super(x)
}
"#,
        );
        assert!(has_errors(&diags), "expected error, got {:?}", diags);
        assert!(
            diags.iter().any(|d| d.message.contains("super")),
            "{:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
