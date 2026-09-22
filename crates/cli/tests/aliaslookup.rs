//! E2E tests for the `agent/aliaslookup` slice: two ways a jar's members go
//! missing or double up, both found while chasing gitbucket's ~220 slick
//! `Session` diagnostics (`docs/gitbucket.md`).
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. The miniature library is `alib`, modelled on
//! `slick.jdbc.JdbcBackend` and on scalatra's `ScalatraContext` /
//! `DynamicScope`. Neither shape reproduces without a jar: a `type` alias
//! leaves no trace in the bytecode, and a pickled member's owner is what the
//! second one turns on.
//!
//! 1. **`object X extends X` lost the trait's own members.**
//!    `SigCache::lin_of` deduplicated its linearization by class *name*, and a
//!    module class carries the same dotted name as its companion. So for
//!
//!    ```scala
//!    trait JdbcBackend extends BaseBackend { type Database = DatabaseDef }
//!    object JdbcBackend extends JdbcBackend
//!    ```
//!
//!    the walk from `JdbcBackend$` put the trait second and then dropped it as
//!    a duplicate of the head. `import JdbcBackend.{Database => …}` found only
//!    `BaseBackend`'s abstract `type Database`, never the alias -- which is
//!    what real scalac calls "`JdbcBackend.Database` (which expands to)
//!    `DatabaseDef`". Slick's cake traits are written this way throughout.
//!
//! 2. **One inherited implicit was offered twice.** `implicits_in_scope` walks
//!    the parents and collects each base's members separately, so a
//!    declaration in one trait stood beside the definition that implements it
//!    in another. nsc's `findMember` sees a single member; we saw two of the
//!    same name and the same type and reported `ambiguous implicit: ctx, ctx`.
//!    scalatra declares `implicit def request: HttpServletRequest` in
//!    `ScalatraContext` and defines it in `DynamicScope` -- unrelated traits,
//!    both mixed into `ScalatraFilter` -- and 169 gitbucket diagnostics were
//!    that pair, with ~219 more downstream of them.
//!
//! `DefFirst` and `DeclFirst` mix the two traits in both orders, because the
//! rule that keeps one of them is "the linearization reaches it first" and
//! both orders have to work.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-aliaslookup-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn jar_tool() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin/jar");
        if p.is_file() {
            return Some(p);
        }
    }
    let out = Command::new("which").arg("jar").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    p.is_file().then_some(p)
}

fn pack_jar(classes: &Path, dest: &Path) {
    let tool = jar_tool().expect("jar tool");
    let out = Command::new(tool)
        .args([
            "cf",
            dest.to_str().unwrap(),
            "-C",
            classes.to_str().unwrap(),
        ])
        .arg(".")
        .output()
        .expect("run jar");
    assert!(
        out.status.success(),
        "jar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_against(out: &Path, jar: &Path, src: &Path, cp: &Path) -> (bool, String) {
    let output = Command::new(bin())
        .arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["-cp", cp.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// The two library shapes, in one file so the fixture costs one scalac run.
const A_LIB: &str = r#"
package alib

class Ctx(val tag: String)

// `org.scalatra.ScalatraContext`: declares the implicit and nothing else.
trait CtxScope {
  implicit def ctx: Ctx
}

// `org.scalatra.DynamicScope`: defines the same member, and is *not* a
// subclass of the trait that declares it.
trait DynScope {
  implicit def ctx: Ctx = new Ctx("dyn")
}

trait DefFirst extends CtxScope with DynScope
trait DeclFirst extends DynScope with CtxScope

// Two implicits of one type that really are ambiguous, so the rule that
// merges a declaration with its definition cannot merge these as well.
trait OneCtx { implicit def one: Ctx = new Ctx("one") }
trait TwoCtx { implicit def two: Ctx = new Ctx("two") }

object U {
  def useCtx(implicit c: Ctx): String = c.tag
}

// `slick.jdbc.JdbcBackend`: an object extending the trait of its own name,
// where the trait turns an inherited abstract type into an alias.
class DatabaseDef { def label: String = "db" }

trait BaseBackend {
  type Database
  def open(): Database
}

trait JdbcBackend extends BaseBackend {
  type Database = DatabaseDef
  def open(): Database = new DatabaseDef
}

object JdbcBackend extends JdbcBackend

trait Writer[A] { def write(value:A):Int }
trait CallableBuilders {
  class Builder {
    def apply(path:String):Int = 0
    def apply[A](path:String, value:A)(implicit writer:Writer[A]):Int = writer.write(value)
  }
  val Submit:Builder = new Builder
  def fresh():Builder = new Builder
}
object CallableBuilders extends CallableBuilders
trait ProjectionBackend {
  class Entry
  type Result = Entry
}
object ProjectionBackend extends ProjectionBackend
trait ProjectionProfile {
  type Backend = ProjectionBackend
  trait API { type Result = Backend#Result }
  val api: API
}
object ProjectionProfile extends ProjectionProfile { val api = new API {} }

abstract class ResultTable[T] { type Element = T }
case class ResultRow(value: Int)
class ConcreteResultTable extends ResultTable[ResultRow]
class ResultBuilder {
  private val buffer = new java.lang.StringBuilder
  def +=(value: String): Unit = { buffer.append(value); () }
  override def toString: String = buffer.toString
}
class ResultContext { val builder = new ResultBuilder }
object ResultExpression {
  def apply(f: ResultContext => Unit): String = {
    val context = new ResultContext
    f(context)
    context.builder.toString
  }
}

final class Projection[R](val value: R)
final class Shape[U](val value: U) {
  def map[R](read: U => R, write: R => Option[U]): Projection[R] = new Projection(read(value))
}

abstract class Chain {
  type Self <: Chain
  type ::[E] = Link[E, Self]
  def self: Self
  def ::[E](e: E): this.::[E] = new Link(e, self)
}
final class Link[H, T <: Chain](val head: H, val tail: T) extends Chain {
  type Self = Link[H, T]
  def self: Self = this
}
object Link {
  def unapply[H, T <: Chain](link: Link[H, T]): Some[(H, T)] = Some((link.head, link.tail))
}
object End extends Chain { type Self = End.type; def self: Self = this }
object Target { def create(s: String): String = s }
trait ExportedAliases { val Target = alib.Target }
package object exported extends ExportedAliases

class DefaultContext(val seed: String) {
  def run(prefix: String)(implicit ctx: Ctx = new Ctx(prefix + seed)): String = ctx.tag
  def only(implicit ctx: Ctx = new Ctx(seed)): String = ctx.tag
}

trait Evidence[A] { def value: A }
trait ExtraEvidence
trait MissingEvidence
object Evidence {
  implicit def stringEvidence: Evidence[String] with ExtraEvidence =
    new Evidence[String] with ExtraEvidence { def value: String = "evidence" }
  def read[A](implicit ev: Evidence[A]): A = ev.value
}

trait PhantomEvidence[A] { def value: String }
object PhantomEvidence {
  implicit def unit[A >: String <: CharSequence]: PhantomEvidence[Unit] =
    new PhantomEvidence[Unit] { def value: String = "phantom" }
}
trait SelfAliases { type Receive = PartialFunction[Any, Unit] }
trait SelfOther

trait Pack[A] { type Out }
class Packed[A]
class DependentMap[A] {
  def map[B](f: A => B)(implicit pack: Pack[B]): Packed[pack.Out] = new Packed[pack.Out]
}

case class Captured[E,A](value: A)
trait Replace[F[_]] { def replace[A,B](fa: F[A], b: B): F[B] }
object Captured {
  implicit def replace[E]: Replace[({ type L[A] = Captured[E,A] })#L] =
    new Replace[({ type L[A] = Captured[E,A] })#L] {
      def replace[A,B](fa: Captured[E,A], b: B): Captured[E,B] = Captured[E,B](b)
    }
}
class ReplaceOps[F[_],A](fa: F[A]) {
  def replacing[B](b: B)(implicit F: Replace[F]): F[B] = F.replace(fa,b)
}
object ReplaceSyntax {
  implicit def ops[F[_],A](fa: F[A]): ReplaceOps[F,A] = new ReplaceOps(fa)
}

object FunctionDefaults {
  var evaluations: Int = 0
  def take(f: () => Long = () => { evaluations += 1; 42L }): Long = f()
}
trait ConstructorEvidence[A] { def label: String }
class Bounded[A: ConstructorEvidence](val value: A)(implicit val suffix: String) {
  def label: String = implicitly[ConstructorEvidence[A]].label + suffix
}
class ParentTransform {
  def transform[A](f: java.util.function.Function[String, java.util.concurrent.CompletionStage[A]]): Int = 1
}
class ChildTransform extends ParentTransform {
  def transform[A](f: Int => Boolean => String => A): Int = 2
}

trait DomainHeader { def label: String }
trait MarkerHeader extends DomainHeader
case class FirstHeader(label: String) extends MarkerHeader
case class SecondHeader(label: String) extends DomainHeader

class NamedPool(val host: String = "localhost", val port: Int = 6379,
                val database: Int = 0, val timeout: Int = 1000) {
  def label: String = host + ":" + port + "/" + database + "@" + timeout
}

class StringOps(val text: String) { def sizeX: Int = text.length }
trait SyntaxBase { implicit def enrich(text: String): StringOps = new StringOps(text) }
object Syntax extends SyntaxBase

trait Mapping[F[_], G[_]] { def apply[A](fa: F[A]): G[A] }
class Box[F[_]](val value: F[Int])
object AppliedAliases {
  type Id[A] = A
  object Id { def apply[A](a: A): A = a }
  def identityBox: Box[Id] = new Box[Id](42)
  type Numbers = List[Int]
  type Pair = (Int, String)
  type Count = Int
}

trait ProtectedBase[A] { protected def value: A; protected def length: Int = -1 }
object ProtectedBase {
  trait Read[A, C] { def read(c: C): A }
  implicit class Ops[A, C](c: C)(implicit r: Read[A, C]) { def length: Int = r.read(c).toString.length }
}
abstract class ProtectedFactory[A, C <: ProtectedBase[A]] {
  final def unapply(c: C): Some[A] = Some(extract(c))
  def extract(c: C): A
  implicit val reader: ProtectedBase.Read[A, C] = new ProtectedBase.Read[A, C] { def read(c: C): A = extract(c) }
}
trait ArrayParser {
  def parse(value: Array[Byte]): String = "bytes:" + value.length
  def parse(value: java.io.InputStream): String = "stream"
  def parse(value: java.io.Reader): String = "reader"
}
object Parser extends ArrayParser
import scala.language.dynamics
object DynamicApi extends scala.Dynamic {
  def known[T](value: T): T = value
  def applyDynamic(name: String)(args: Any*): String = name
}

trait RefinedEvidence { type T; def tag: String }
object RefinedEvidence {
  type Aux[A] = RefinedEvidence { type T = A }
  implicit val specific: Aux[Int] = new RefinedEvidence { type T = Int; def tag = "specific" }
  implicit def generic[A]: Aux[A] = new RefinedEvidence { type T = A; def tag = "generic" }
}

trait InvariantBox[A]
trait Element { type Self <: Element; def companion: InvariantBox[Self] }
trait IsTuple[A]
object IsTuple { implicit def one[A]: IsTuple[Tuple1[A]] = new IsTuple[Tuple1[A]] {} }
trait TupleEvidence[A] { type Out; def apply(a: A): Out }
trait FallbackEvidence {
  implicit def single[A]: TupleEvidence[A] { type Out = Tuple1[A] } =
    new TupleEvidence[A] { type Out = Tuple1[A]; def apply(a: A) = Tuple1(a) }
}
object TupleEvidence extends FallbackEvidence {
  implicit def tuple[A](implicit ev: IsTuple[A]): TupleEvidence[A] { type Out = A } =
    new TupleEvidence[A] { type Out = A; def apply(a: A) = a }
}

trait UnaryFunction {
  def apply[A, B](value: A)(implicit impl: UnaryFunction.Impl[this.type, A, B]): B = impl(value)
}
object UnaryFunction { trait Impl[Tag, A, B] { def apply(value: A): B } }
object DoubleFunction extends UnaryFunction {
  implicit object DoubleImpl extends UnaryFunction.Impl[DoubleFunction.type, Double, Double] {
    def apply(value: Double): Double = value * 2
  }
}

trait AbstractEncoding {
  trait Tag
  type Type[A] <: Tag
}
object EncodedValue extends AbstractEncoding {
  def create[A](value: A): Type[A] = new Box(value).asInstanceOf[Type[A]]
  class Box[A](val value: A) extends Tag
  class Ops[A](encoded: Type[A]) { def underlying: A = encoded.asInstanceOf[Box[A]].value }
  implicit def valueOps[A](value: Type[A]): Ops[A] = new Ops(value)
}
object EncodedAliases { type Value[A] = EncodedValue.Type[A] }
trait OperatorResult { type Out; def value:Out }
object DependentOperator {
  def /(value:Int)(implicit result:OperatorResult):result.Out = result.value
}

class NestedViewResult
class NestedViewTarget(val value: Int)
object NestedViewResult {
  implicit def convert(value: Int => Option[NestedViewResult]): NestedViewTarget =
    new NestedViewTarget(42)
}
object NestedViewReceiver {
  def accept(value: NestedViewTarget): Int = value.value
}

"#;

/// gitbucket's `TransactionFilter` renames the alias exactly like this.
const A_USER: &str = r#"
import alib.JdbcBackend.{Database => MyDatabase}

class AliasUser {
  val db: MyDatabase = alib.JdbcBackend.open()
  def dbLabel: String = db.label
}

class DefFirstUser extends alib.DefFirst { def tag: String = alib.U.useCtx }
class DeclFirstUser extends alib.DeclFirst { def tag: String = alib.U.useCtx }

object Main {
  def main(args: Array[String]): Unit = {
    println(new AliasUser().dbLabel)
    println(new DefFirstUser().tag)
    println(new DeclFirstUser().tag)
  }
}
"#;

/// Reading the alias is not the same as accepting anything under its name,
/// and merging a declaration with its definition is not the same as merging
/// two implicits. Real scalac reports both of these too.
const A_USER_BAD: &str = r#"
import alib.JdbcBackend.{Database => MyDatabase}

class BadAlias {
  val db: MyDatabase = "not a database"
}

class BadAmbiguous extends alib.OneCtx with alib.TwoCtx {
  def tag: String = alib.U.useCtx
}
"#;

const A_EXPECTED: &str = "db\ndyn\ndyn\n";

fn build_lib_jar(dir: &Path) -> PathBuf {
    let src = dir.join("alib.scala");
    fs::write(&src, A_LIB).unwrap();
    let package = dir.join("package.scala");
    fs::write(
        &package,
        "package object alib { type ~>[F[_], G[_]] = alib.Mapping[F, G] }",
    )
    .unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let scalac = self::scalac().expect("checked by caller");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&src)
        .arg(&package)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("alib.jar");
    pack_jar(&lib_out, &lib_jar);
    lib_jar
}

/// A pickled alias declared by a trait its own companion extends, and an
/// inherited implicit declared by one trait and defined by another.
#[test]
fn a_companions_alias_and_a_doubled_inherited_implicit() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    if scalac().is_none() {
        eprintln!("skip: no scalac to write the pickle with");
        return;
    }
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("alias");
    let lib_jar = build_lib_jar(&dir);

    let user = dir.join("user.scala");
    fs::write(&user, A_USER).unwrap();
    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, &lib_jar);
    assert!(ok, "the fixture failed to compile against the jar:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let bad = dir.join("bad.scala");
    fs::write(&bad, A_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let (ok, msgs) = compile_against(&bad_out, &jar, &bad, &lib_jar);
    assert!(!ok, "expected both bad cases to be rejected:\n{msgs}");
    assert!(
        msgs.contains("required: DatabaseDef"),
        "expected the alias to be reported as the `DatabaseDef` it expands to, got:\n{msgs}"
    );
    assert!(
        msgs.contains("ambiguous implicit: one, two"),
        "expected two genuinely ambiguous implicits still to be ambiguous, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(
            run_java(&user_out, &cp),
            A_EXPECTED,
            "stdout mismatch for a_companions_alias_and_a_doubled_inherited_implicit"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The fixture is ordinary Scala, not a quirk of ours: real scalac compiles
/// the same two files, prints the same thing, and rejects the same two
/// programs.
#[test]
fn real_scalac_agrees_on_both_fixtures() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip: no scalac");
        return;
    };
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let dir = tmp_dir("alias-scalac");
    let lib = dir.join("alib.scala");
    let user = dir.join("user.scala");
    fs::write(&lib, A_LIB).unwrap();
    fs::write(&user, A_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&out)
        .args([&lib, &user])
        .output()
        .expect("run scalac");
    assert!(
        result.status.success(),
        "scalac rejected the fixture:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(run_java(&out, jar.to_str().unwrap()), A_EXPECTED);

    let bad = dir.join("bad.scala");
    fs::write(&bad, A_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&bad_out)
        .arg("-cp")
        .arg(&out)
        .arg(&bad)
        .output()
        .expect("run scalac");
    let msgs = String::from_utf8_lossy(&result.stderr).into_owned();
    assert!(!result.status.success(), "scalac accepted the bad file");
    assert!(
        msgs.contains("(which expands to)") && msgs.contains("ambiguous implicit values"),
        "scalac reported something else:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Importing a nullary alias retains its applied RHS, including rejection of
/// incompatible element types. The provider's declarations exist only in its
/// ScalaSignature, not as JVM members.
#[test]
fn imported_alias_keeps_type_arguments() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    if jar_tool().is_none() {
        return;
    }
    let dir = tmp_dir("applied-alias");
    let lib = build_lib_jar(&dir);
    for (label, body, accepted) in [
        ("good", "val xs: Numbers = List(1, 2); val p: Pair = (xs.sum, \"ok\"); val n: Count = p._1; println(n)", true),
        ("bad", "val xs: Numbers = List(\"wrong\")", false),
        ("bad_pair", "val p: Pair = (\"wrong\", 1)", false),
        ("bad_arity", "val xs: Numbers[String] = List(\"wrong\")", false),
        ("identity", "val b: alib.Box[Id] = identityBox; println(b.value - 39)", true),
        ("identity_bad", "val b: alib.Box[List] = identityBox", false),
        ("operator_alias", "import alib.~>; val f: List ~> Option = new alib.Mapping[List, Option] { def apply[A](xs: List[A]): Option[A] = xs.headOption }; println(f(List(3)).get)", true),
        ("import_prefix", "import alib.Syntax.enrich; println(\"abc\".sizeX)", true),
    ] {
        let source = dir.join(format!("{label}.scala"));
        fs::write(&source, format!("import alib.AppliedAliases.{{Numbers, Pair, Count, Id, identityBox}}\nobject Main {{ def main(args: Array[String]): Unit = {{ {body} }} }}")).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &source, &lib)
            } else {
                let output = Command::new(&scalac).arg("-cp").arg(&lib).arg("-d").arg(&out).arg(&source).output().unwrap();
                (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted && java_available() {
                assert_eq!(run_java(&out, &format!("{}:{}", jar.display(), lib.display())), "3\n");
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn java_generic_method_override_retains_upper_bound() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    if Command::new("javac").arg("-version").output().is_err() {
        return;
    }
    let dir = tmp_dir("java-upper-bound");
    let java = dir.join("Bounded.java");
    fs::write(
        &java,
        "public interface Bounded { <T extends Number> T value(T x); }",
    )
    .unwrap();
    let provider = dir.join("provider");
    fs::create_dir_all(&provider).unwrap();
    assert!(Command::new("javac")
        .arg("-d")
        .arg(&provider)
        .arg(&java)
        .status()
        .unwrap()
        .success());
    for (name, bound, accepted) in [
        ("good", "Number", true),
        ("narrow", "java.lang.Integer", false),
    ] {
        let src = dir.join(format!("{name}.scala"));
        fs::write(&src, format!("class Impl extends Bounded {{ override def value[T <: {bound}](x: T): T = x }}\nobject Main {{ def main(args: Array[String]): Unit = {{ val b: Bounded = new Impl; println(b.value(java.lang.Integer.valueOf(42))) }} }}")).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{name}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &provider)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&provider)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{name}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), provider.display())),
                    "42\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn single_clause_binary_constructor_names_and_defaults() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    if jar_tool().is_none() {
        return;
    }
    let dir = tmp_dir("ctor-names");
    let lib = build_lib_jar(&dir);
    for (label, call, accepted) in [
        (
            "good",
            "new alib.NamedPool(timeout = 42, host = \"remote\", database = 2)",
            true,
        ),
        ("unknown", "new alib.NamedPool(missing = 42)", false),
        ("wrong_type", "new alib.NamedPool(port = \"bad\")", false),
        (
            "duplicate",
            "new alib.NamedPool(host = \"a\", host = \"b\")",
            false,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                "object Main {{ def main(args: Array[String]): Unit = println(({call}).label) }}"
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "remote:6379/2@42\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn inherited_object_alias_uses_concrete_member() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("object-alias");
    for (label, input, accepted) in [
        ("good", "List(1, 2)", true),
        ("wrong", "List(\"bad\")", false),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
trait Family {{ type Elem; type Items = List[Elem] }}
object Ints extends Family {{ type Elem = Int }}
object Strings extends Family {{ type Elem = String }}
object Main {{
  def total(xs: Ints.Items): Int = xs.sum
  def text(xs: Strings.Items): String = xs.mkString
  def main(args: Array[String]): Unit = println(total({input}).toString + text(List("ok")))
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &jar)
            } else {
                let o = Command::new(&scalac)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(run_java(&out, jar.to_str().unwrap()), "3ok\n");
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn implicit_clause_substitutes_dependent_member() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("dependent-implicit");
    for (label, result, accepted) in [("good", "Int", true), ("wrong", "String", false)] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, format!(r#"
import scala.reflect.ClassTag
trait Family {{ type Elem }}
object Ints extends Family {{ type Elem = Int }}
object Main {{
  def array(f: Family)(implicit tag: ClassTag[f.Elem]): Array[f.Elem] = new Array[f.Elem](2)
  def main(args: Array[String]): Unit = {{ val a: Array[{result}] = array(Ints); println(a.length) }}
}}
"#)).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &jar)
            } else {
                let o = Command::new(&scalac)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(run_java(&out, jar.to_str().unwrap()), "2\n");
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

fn source_case(label: &str, source: &str, accepted: bool, expected: &str) {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir(label);
    let src = dir.join("Main.scala");
    fs::write(&src, source).unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &jar)
        } else {
            let o = Command::new(&scalac)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
        if accepted {
            assert_eq!(run_java(&out, jar.to_str().unwrap()), expected);
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn context_bound_applies_captured_type_lambda() {
    let source = r#"
trait Operation[A, F[_], C]
object Operation {
  type With[A, F[_]] = { type T[C] = Operation[A, F, C] }
  def get[A, F[_], C](implicit o: Operation[A, F, C]): Operation[A, F, C] = o
}
class Ops[A, F[_], C: Operation.With[A, F]#T](c: C) {
  def op: Operation[A, F, C] = Operation.get[A, F, C]
}
object Main { def main(args: Array[String]): Unit = {
  implicit val ev: Operation[Int, List, String] = new Operation[Int, List, String] {}
  println(new Ops[Int, List, String]("a").op eq ev)
} }
"#;
    source_case("context-projection", source, true, "true\n");
    source_case(
        "context-projection-bad",
        &source.replace("implicit val ev", "val ev"),
        false,
        "",
    );
}

#[test]
fn higher_kinded_mutable_set_retains_receiver_type() {
    source_case(
        "factory-mutable-set",
        r#"
import scala.collection.{IterableFactory, mutable}
object Main {
  def empty[F[X] <: mutable.Set[X]](factory: IterableFactory[F]): F[Int] = factory.empty[Int]
  def remove[F[X] <: mutable.Set[X]](xs: F[Int]): Option[F[Int]] = Some(xs).map(_ -= 1)
  def main(args: Array[String]): Unit = {
    val xs: mutable.HashSet[Int] = empty(mutable.HashSet)
    xs += 1; xs += 2
    val result: Option[mutable.HashSet[Int]] = remove(xs)
    println(result.get eq xs)
    println(result.get.sum)
  }
}
"#,
        true,
        "true\n2\n",
    );
}

#[test]
fn lambda_body_instantiates_implicit_only_method() {
    source_case(
        "lambda-implicit-only",
        r#"
trait App[F[_]] { def pure[A](a: A): F[A] }
object Main {
  implicit class ValueOps[A](a: A) { def pure[F[_]](implicit app: App[F]): F[A] = app.pure(a) }
  implicit val app: App[List] = new App[List] { def pure[A](a: A): List[A] = List(a) }
  def main(args: Array[String]): Unit = {
    val values = List(1, 2).map(_.pure)
    println(values.flatten.sum)
  }
}
"#,
        true,
        "3\n",
    );
}

#[test]
fn seq_view_flatmap_uses_iterable_result() {
    let source = r#"
object Main {
  def add(xs: Seq[Int], flag: Boolean): Set[Int] = Set.empty[Int] ++ {
    if (flag) xs.map { case i => i + 1 }.toSet
    else for { x <- xs.view; y <- Seq(x, x + 1) } yield y
  }
  def main(args: Array[String]): Unit = {
    println(add(Seq(1, 2), true).sum)
    println(add(Seq(1, 2), false).sum)
  }
}
"#;
    source_case("seqview-flatmap", source, true, "5\n6\n");
    source_case(
        "seqview-flatmap-bad",
        "object Main { val bad = Seq(1).view.flatMap(x => x) }",
        false,
        "",
    );
}

#[test]
fn parameterless_lower_bound_is_read_at_receiver() {
    let source = r#"
object Main {
  val warm = Seq(1).toSet[AnyVal]
  def convert(xs: Seq[String]): IterableOnce[String] = xs.toSet
  def main(args: Array[String]): Unit = println(convert(Seq("ok", "ok")).iterator.mkString)
}
"#;
    source_case("parameterless-bound", source, true, "ok\n");
    source_case(
        "parameterless-bound-bad",
        &source.replace("IterableOnce[String] =", "IterableOnce[Int] ="),
        false,
        "",
    );
}

#[test]
fn case_copy_default_keeps_value_class_representation() {
    source_case(
        "value-class-copy-default",
        r#"
case class Key(value: String) extends AnyVal
case class Number(value: Int) extends AnyVal
trait Context { val key: Key; val number: Number }
case class Row(override val key: Key, override val number: Number) extends Context
object Main {
  def main(args: Array[String]): Unit = {
    val row = Row(Key("ok"), Number(42))
    val copied = row.copy()
    println(copied.key.value + copied.number.value)
  }
}
"#,
        true,
        "ok42\n",
    );
}

#[test]
fn binary_lambda_result_is_not_fixed_by_contravariant_sibling() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("binary-lambda-result");
    let lib = build_lib_jar(&dir);
    for (label, result, write, accepted) in [
        ("wide-input", "Option[Int]", "Any", true),
        ("exact-input", "Option[Int]", "Option[Int]", true),
        ("wrong-result", "Option[String]", "Any", false),
        ("narrow-input", "Option[Int]", "String", false),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
object Main {{ def main(args: Array[String]): Unit = {{
  val p: alib.Projection[{result}] = new alib.Shape(Option(42)).map(
    r => r.map(_ + 1), (_: {write}) => throw new Exception("unused"))
  println(p.value.get)
}} }}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "43\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

/// Encoded type aliases retain the receiver's concrete Self, and a stable
/// inherited package accessor replaces its erased classfile forwarder.
#[test]
fn binary_symbolic_alias_and_reexported_object_match_scalac() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("symbolic-alias");
    let lib = build_lib_jar(&dir);
    for (label, head, accepted) in [("valid", "Int", true), ("wrong-head", "String", false)] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
import alib.End
import alib.exported.Target
object Main {{ def main(args: Array[String]): Unit = {{
  val xs: alib.Link[{head}, alib.Link[String, End.type]] = 42 :: "ok" :: End
  println(xs.head)
  println(xs.tail.head)
  println(Target.create("alias"))
  xs match {{
    case alib.Link(i, alib.Link(s, End)) =>
      val head: Int = i
      val tail: String = s
      println(head + tail.length)
  }}
}} }}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "42\nok\nalias\n44\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn binary_implicit_defaults_use_getters_and_prefer_supplied_values() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("implicit-default");
    let lib = build_lib_jar(&dir);
    for (label, source, expected) in [
        (
            "valid",
            r#"
object Main {
  val instance = new alib.DefaultContext("seed")
  def defaults(): String = instance.run("prefix-") + ":" + instance.only
  def supplied(): String = {
    implicit val ctx: alib.Ctx = new alib.Ctx("provided")
    instance.run("ignored") + ":" + instance.only
  }
  def main(args: Array[String]): Unit = { println(defaults()); println(supplied()) }
}
"#,
            Some("prefix-seed:seed\nprovided:provided\n"),
        ),
        (
            "ambiguous",
            r#"
object Main {
  implicit val first: alib.Ctx = new alib.Ctx("one")
  implicit val second: alib.Ctx = new alib.Ctx("two")
  val value = new alib.DefaultContext("fallback").only
}
"#,
            None,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, expected.is_some(), "{label}, ours={ours}: {diagnostic}");
            if let Some(expected) = expected {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    expected
                );
            } else {
                assert!(diagnostic.contains("ambiguous implicit"), "{diagnostic}");
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn intersection_witness_infers_open_target_without_losing_requirements() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("intersection-evidence");
    let lib = build_lib_jar(&dir);
    for (label, expression, accepted) in [
        (
            "inferred",
            "val inferred = alib.Evidence.read; val result: String = inferred; println(result)",
            true,
        ),
        ("wrong-element", "implicitly[alib.Evidence[Int]]", false),
        (
            "missing-parent",
            "implicitly[alib.Evidence[String] with alib.MissingEvidence]",
            false,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                "object Main {{ def main(args: Array[String]): Unit = {{ {expression}; () }} }}"
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "evidence\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn phantom_implicit_parameters_and_binary_self_aliases() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("phantom-self-alias");
    let lib = build_lib_jar(&dir);
    for (label, source, expected) in [
        (
            "valid",
            r#"
trait Handler { self: alib.SelfAliases with alib.SelfOther =>
  def receive: Receive = { case _ => () }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[alib.PhantomEvidence[Unit]].value)
    val h = new Handler with alib.SelfAliases with alib.SelfOther
    h.receive("ok")
    println(h.receive.isDefinedAt(1))
  }
}
"#,
            Some("phantom\ntrue\n"),
        ),
        (
            "wrong-evidence",
            "object Main { implicitly[alib.PhantomEvidence[String]] }",
            None,
        ),
        (
            "wrong-alias",
            r#"
trait Handler { self: alib.SelfAliases with alib.SelfOther =>
  def receive: Receive = 1
}
"#,
            None,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, expected.is_some(), "{label}, ours={ours}: {diagnostic}");
            if let Some(expected) = expected {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    expected
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn partial_function_result_can_widen_the_receiver_lower_bound() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("recover-result");
    for (label, result, accepted) in [
        ("widen", "Either[String, Int]", true),
        ("narrow", "Right[Nothing, Int]", false),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, format!(r#"
import scala.concurrent.{{Await, ExecutionContext, Future}}
import scala.concurrent.duration.Duration
object Main {{
  implicit val ec: ExecutionContext = ExecutionContext.global
  def recover(f: Future[Right[Nothing, Int]]): Future[{result}] =
    f.recover {{ case _: IllegalArgumentException => Left("bad") }}
  def main(args: Array[String]): Unit = {{
    println(Await.result(recover(Future.successful(Right(1))), Duration(10, "seconds")))
    println(Await.result(recover(Future.failed(new IllegalArgumentException)), Duration(10, "seconds")))
  }}
}}
"#)).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &jar)
            } else {
                let o = Command::new(&scalac)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, jar.to_str().unwrap()),
                    "Right(1)\nLeft(bad)\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn curried_binary_results_keep_the_actual_dependent_path() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("curried-dependent-path");
    let lib = build_lib_jar(&dir);
    for (label, result_path, accepted) in [("same", "p", true), ("different", "q", false)] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, format!(r#"
object Main {{
  def map[A,B](d: alib.DependentMap[A], f: A => B)(p: alib.Pack[B], q: alib.Pack[B]): alib.Packed[{result_path}.Out] = d.map(f)(p)
  def main(args: Array[String]): Unit = {{
    val p = new alib.Pack[String] {{ type Out = String }}
    val result: alib.Packed[String] = map(new alib.DependentMap[Int], (i: Int) => i.toString)(p,p)
    println(result != null)
  }}
}}
"#)).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "true\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn standard_optional_value_class_conversions_run() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("optional-converters");
    for (label, typ, accepted) in [("valid", "String", true), ("invalid", "Int", false)] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
import scala.jdk.OptionConverters._
object Main {{
  def main(args: Array[String]): Unit = {{
    val value: Option[{typ}] = java.util.Optional.of("hello").toScala
    println(value)
    println(java.util.Optional.empty[String]().toScala)
    println(Option("roundtrip").toJava.get())
  }}
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &jar)
            } else {
                let o = Command::new(&scalac)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, jar.to_str().unwrap()),
                    "Some(hello)\nNone\nroundtrip\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn binary_type_lambdas_capture_parameters_for_syntax_inference() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("captured-type-lambda");
    let lib = build_lib_jar(&dir);
    for (label, error_type, accepted) in [
        ("captured", "String", true),
        ("wrong-capture", "Boolean", false),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
import alib.ReplaceSyntax._
object Main {{
  def main(args: Array[String]): Unit = {{
    val start = alib.Captured[String,Int](42)
    val result: alib.Captured[{error_type},String] = start.replacing("done")
    println(result.value)
  }}
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let o = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "done\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn branch_common_bases_and_byname_conversions_match_scalac() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("branch-byname");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(&src, r#"
import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.duration.Duration
object Main {
  def headers(xs: Seq[alib.DomainHeader]) = {
    val result = xs.map {
      case alib.FirstHeader(s) => alib.FirstHeader(s + "1")
      case alib.SecondHeader(s) => alib.SecondHeader(s + "2")
      case h => h
    }
    val checked: Seq[alib.DomainHeader] = result
    checked
  }
  def delayed(x: java.lang.Boolean)(implicit ec: ExecutionContext): Future[Boolean] = Future[Boolean] { x }
  def main(args: Array[String]): Unit = {
    println(headers(List(alib.FirstHeader("a"), alib.SecondHeader("b"))).map(_.label).mkString(","))
    implicit val ec: ExecutionContext = ExecutionContext.global
    println(Await.result(delayed(java.lang.Boolean.TRUE), Duration(10, "seconds")))
  }
}
"#).unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "a1,b2\ntrue\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn binary_declaring_owner_orders_unrelated_function_parameters() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("declaring-owner");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(
        &src,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(new alib.ChildTransform().transform(_ => _ => s => s.length))
  }
}
"#,
    )
    .unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "2\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn binary_function_defaults_and_context_bound_constructor_order() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("default-context-bound");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(&src, r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(alib.FunctionDefaults.take())
    println(alib.FunctionDefaults.evaluations)
    implicit val evidence: alib.ConstructorEvidence[Int] = new alib.ConstructorEvidence[Int] { def label: String = "int" }
    implicit val suffix: String = "!"
    println(new alib.Bounded[Int](42).label)
    class Derived[A: alib.ConstructorEvidence](value: A)(implicit suffix: String) extends alib.Bounded[A](value)
    println(new Derived[Int](42).label)
  }
}
"#).unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "42\n1\nint!\nint!\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ordered_parent_and_dependent_shuffle_builder() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("ordered-shuffle");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(&src, r#"
object Main {
  def main(args: Array[String]): Unit = {
    case class Item(value: Int) extends Ordered[Item] { def compare(other: Item): Int = value.compare(other.value) }
    println(scala.collection.immutable.SortedSet(Item(2), Item(1)).toList.map(_.value))
    println(scala.util.Random.shuffle(Seq(1, 2)).size)
  }
}
"#).unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "List(1, 2)\n2\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn protected_member_uses_inherited_companion_witness() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("protected-witness");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(&src, r#"
case class Wrapper(protected val value: String) extends alib.ProtectedBase[String] { def content: String = value }
object Wrapper extends alib.ProtectedFactory[String, Wrapper] { def extract(c: Wrapper): String = c.content }
object Noise {
  trait Missing[A]
  implicit class Unavailable[A](a: A)(implicit ev: Missing[A]) { def length: Int = -1 }
}
import Noise._
object Main { def main(args: Array[String]): Unit = { println(Wrapper("hello").length); println(Array("a", "b").lift(1)) } }

"#).unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "5\nSome(b)\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn protected_member_uses_inherited_companion_witness_after_separate_compilation() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("protected-witness-separate");
    let lib = build_lib_jar(&dir);
    let wrapper_src = dir.join("Wrapper.scala");
    fs::write(
        &wrapper_src,
        r#"
package nested

case class Wrapper(protected val value: String) extends alib.ProtectedBase[String] {
  def content: String = value
}
object Wrapper extends alib.ProtectedFactory[String, Wrapper] {
  def extract(c: Wrapper): String = c.content
}
"#,
    )
    .unwrap();
    let main_src = dir.join("Main.scala");
    fs::write(
        &main_src,
        r#"
import nested.Wrapper

object Main {
  def main(args: Array[String]): Unit = println(new Wrapper("hello").length)
}
"#,
    )
    .unwrap();

    let compile = |ours: bool, out: &Path, src: &Path, cp: &str| {
        if ours {
            let output = Command::new(bin())
                .arg("compile")
                .arg(src)
                .arg("-d")
                .arg(out)
                .arg("-cp")
                .arg(cp)
                .arg("--scala-library")
                .arg(&jar)
                .output()
                .unwrap();
            (
                output.status.success(),
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stderr),
                    String::from_utf8_lossy(&output.stdout)
                ),
            )
        } else {
            let output = Command::new(&scalac)
                .arg("-cp")
                .arg(cp)
                .arg("-d")
                .arg(out)
                .arg(src)
                .output()
                .unwrap();
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        }
    };

    for producer in [false, true] {
        let wrapper_out = dir.join(format!("wrapper-{producer}"));
        fs::create_dir_all(&wrapper_out).unwrap();
        let (ok, diagnostic) = compile(producer, &wrapper_out, &wrapper_src, lib.to_str().unwrap());
        assert!(ok, "producer={producer}: {diagnostic}");

        for consumer in [false, true] {
            let main_out = dir.join(format!("main-{producer}-{consumer}"));
            fs::create_dir_all(&main_out).unwrap();
            let cp = format!("{}:{}", lib.display(), wrapper_out.display());
            let (ok, diagnostic) = compile(consumer, &main_out, &main_src, &cp);
            assert!(ok, "producer={producer}, consumer={consumer}: {diagnostic}");
            assert_eq!(
                run_java(
                    &main_out,
                    &format!(
                        "{}:{}:{}",
                        wrapper_out.display(),
                        jar.display(),
                        lib.display()
                    )
                ),
                "5\n"
            );
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn qualified_higher_kind_alias_survives_separate_compilation() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("qualified-hk-alias-separate");
    let domain_src = dir.join("Domain.scala");
    fs::write(
        &domain_src,
        r#"
object Domain {
  type Lookup[F[_]] = Int => F[Boolean]
}
"#,
    )
    .unwrap();
    let repo_src = dir.join("Repo.scala");
    fs::write(
        &repo_src,
        r#"
object Repo {
  type Lookup[F[_]] = String => Domain.Lookup[F]
  def result: Lookup[Option] = text => number => Some(text.length == number)
}

object Main {
  def main(args: Array[String]): Unit = println(Repo.result("ok")(2))
}
"#,
    )
    .unwrap();

    let compile = |ours: bool, out: &Path, src: &Path, cp: Option<&str>| {
        let output = if ours {
            let mut command = Command::new(bin());
            command
                .arg("compile")
                .arg(src)
                .arg("-d")
                .arg(out)
                .arg("--scala-library")
                .arg(&jar);
            if let Some(cp) = cp {
                command.arg("-cp").arg(cp);
            }
            command.output().unwrap()
        } else {
            let mut command = Command::new(&scalac);
            if let Some(cp) = cp {
                command.arg("-cp").arg(cp);
            }
            command.arg("-d").arg(out).arg(src);
            command.output().unwrap()
        };
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ),
        )
    };

    for producer_ours in [false, true] {
        let producer_out = dir.join(format!("domain-{producer_ours}"));
        fs::create_dir_all(&producer_out).unwrap();
        let (ok, diagnostic) = compile(producer_ours, &producer_out, &domain_src, None);
        assert!(ok, "producer={producer_ours}: {diagnostic}");

        for consumer_ours in [false, true] {
            let consumer_out = dir.join(format!("repo-{producer_ours}-{consumer_ours}"));
            fs::create_dir_all(&consumer_out).unwrap();
            let cp = producer_out.to_str().unwrap();
            let (ok, diagnostic) = compile(consumer_ours, &consumer_out, &repo_src, Some(cp));
            assert!(
                ok,
                "producer={producer_ours}, consumer={consumer_ours}: {diagnostic}"
            );
            assert_eq!(
                run_java(
                    &consumer_out,
                    &format!("{}:{}", producer_out.display(), jar.display())
                ),
                "Some(true)\n"
            );
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn named_copy_preserves_separately_compiled_implicit_conversion() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("named-copy-implicit-separate");
    let producer_src = dir.join("Record.scala");
    fs::write(
        &producer_src,
        r#"
sealed trait Situation
final case class ReadTime(value: Float)

case class Record(value: Int) {
  def update(xs: Seq[Situation], read: ReadTime): Record =
    copy(value = read.value.toInt)
}

object Record {
  implicit class SeqRecordOps(private val records: Seq[Record]) {
    def filterWithBackTrans: Seq[Record] = records.filter(_.value > 0)
    def replaceByKey[Key](newRecord: Record)(keyOf: Record => Key): Seq[Record] =
      records.map(_ => newRecord)
  }
}
"#,
    )
    .unwrap();
    let consumer_src = dir.join("Main.scala");
    fs::write(
        &consumer_src,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val records: Seq[Record] = Seq(Record(1))
    println(records.filterWithBackTrans.size)
    println(records.replaceByKey(Record(2))(_.value).head.value)
  }
}
"#,
    )
    .unwrap();

    let compile = |ours: bool, out: &Path, src: &Path, cp: Option<&str>| {
        let output = if ours {
            let mut command = Command::new(bin());
            command
                .arg("compile")
                .arg(src)
                .arg("-d")
                .arg(out)
                .arg("--scala-library")
                .arg(&jar);
            if let Some(cp) = cp {
                command.arg("-cp").arg(cp);
            }
            command.output().unwrap()
        } else {
            let mut command = Command::new(&scalac);
            if let Some(cp) = cp {
                command.arg("-cp").arg(cp);
            }
            command.arg("-d").arg(out).arg(src);
            command.output().unwrap()
        };
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ),
        )
    };

    for producer_ours in [false, true] {
        let producer_out = dir.join(format!("record-{producer_ours}"));
        fs::create_dir_all(&producer_out).unwrap();
        let (ok, diagnostic) = compile(producer_ours, &producer_out, &producer_src, None);
        assert!(ok, "producer={producer_ours}: {diagnostic}");

        for consumer_ours in [false, true] {
            let consumer_out = dir.join(format!("main-{producer_ours}-{consumer_ours}"));
            fs::create_dir_all(&consumer_out).unwrap();
            let cp = producer_out.to_str().unwrap();
            let (ok, diagnostic) = compile(consumer_ours, &consumer_out, &consumer_src, Some(cp));
            assert!(
                ok,
                "producer={producer_ours}, consumer={consumer_ours}: {diagnostic}"
            );
            assert_eq!(
                run_java(
                    &consumer_out,
                    &format!("{}:{}", producer_out.display(), jar.display())
                ),
                "1\n2\n"
            );
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn inherited_array_overloads_and_real_dynamic_members_match_scalac() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("array-overload");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(
        &src,
        r#"
object Main { def main(args: Array[String]): Unit = {
  println(alib.Parser.parse(Array[Byte](1, 2)))
  println(alib.DynamicApi.known(42))
  println(implicitly[alib.RefinedEvidence.Aux[Int]].tag)
  println(implicitly[alib.RefinedEvidence.Aux[String]].tag)
} }
"#,
    )
    .unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "bytes:2\n42\nspecific\ngeneric\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn abstract_override_and_correlated_implicit_results_match_scalac() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("abstract-correlated");
    let lib = build_lib_jar(&dir);
    let src = dir.join("Main.scala");
    fs::write(
        &src,
        r#"
trait DerivedElement extends alib.Element {
  type Self = DerivedElement
  def companion: alib.InvariantBox[DerivedElement]
}
object Main {
  def choose[A](a: A)(implicit ev: alib.TupleEvidence[A]): ev.Out = ev(a)
  def main(args: Array[String]): Unit = {
    println(choose(Tuple1("ok")))
    println(choose("value"))
    println(alib.DoubleFunction(2.0))
    import alib.DoubleFunction
    println(DoubleFunction(3.0))
  }
}
"#,
    )
    .unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "(ok)\n(value)\n4.0\n6.0\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn native_universal_trait_accepts_value_class_clients() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("universal-trait");
    let lib = dir.join("lib");
    fs::create_dir_all(&lib).unwrap();
    let producer = dir.join("Universal.scala");
    fs::write(
        &producer,
        "package universal; trait Value extends Any { def value: Int; def next: Int = value + 1 }",
    )
    .unwrap();
    let (ok, diagnostic) = compile_against(&lib, &jar, &producer, &jar);
    assert!(ok, "{diagnostic}");
    let src = dir.join("Main.scala");
    fs::write(
        &src,
        r#"
class Number(val value: Int) extends AnyVal with universal.Value
object Main { def main(args: Array[String]): Unit = println(new Number(41).next) }
"#,
    )
    .unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &lib)
        } else {
            let o = Command::new(&scalac)
                .arg("-cp")
                .arg(&lib)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
            "42\n"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn literal_views_partial_functions_and_invariant_branch_bounds_match_scalac() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("literal-adaptation");
    let src = dir.join("Main.scala");
    fs::write(&src, r#"
import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.duration.{Duration, DurationDouble}
trait Algebra[F[_]] { def pure[A](a: A): F[A] }
class Box[A](val value: A)
object Box extends Algebra[Box] { def pure[A](a: A): Box[A] = new Box(a) }
trait Branches[F[_]] {
  def source: F[Seq[Int]]
  def choose(flag: Boolean)(implicit F: Algebra[F]) = if (flag) source else F.pure(Seq.empty)
  def bounded(flag: Boolean)(implicit F: Algebra[F]): F[_ <: Seq[Int]] = choose(flag)
}
object Main extends Branches[Box] {
  def source = Box.pure(Seq(42))
  implicit val ec: ExecutionContext = ExecutionContext.global
  def recovered: Future[Int] = Future.failed[Int](new Exception).recoverWith { _ => Future.successful(42) }
  def ready(f: Future[Int]): Future[Int] = Await.ready(f, Duration.Inf)
  def nested: Future[(Int, String)] = for {
    values <- Future.successful(Seq(1))
    pair <- Future.failed[(Int, String)](new Exception).recoverWith { _ =>
      Future.successful(values).flatMap(_ => Future.successful((42, "ok")))
    }
  } yield pair
  def main(args: Array[String]): Unit = {
    println(1.day.toHours)
    println(3.days.toHours)
    println(bounded(true)(Box).value)
    println(bounded(false)(Box).value)
    val total: PartialFunction[Int, Int] = x => x match { case 1 => 42 }
    println(total.isDefinedAt(2))
    println(total(1))
    println(total.applyOrElse(1, (_: Int) => -1))
    println(Await.result(ready(recovered), Duration.Inf))
    val lookup: PartialFunction[Int, String] = List(1 -> "ok").toMap
    println(lookup(1))
    println(Await.result(nested, Duration.Inf))
  }
}
"#).unwrap();
    for ours in [false, true] {
        let out = dir.join(format!("out-{ours}"));
        fs::create_dir_all(&out).unwrap();
        let (ok, diagnostic) = if ours {
            compile_against(&out, &jar, &src, &jar)
        } else {
            let o = Command::new(&scalac)
                .arg("-d")
                .arg(&out)
                .arg(&src)
                .output()
                .unwrap();
            (
                o.status.success(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        };
        assert!(ok, "ours={ours}: {diagnostic}");
        assert_eq!(
            run_java(&out, &jar.display().to_string()),
            "24\n72\nList(42)\nList()\nfalse\n42\n42\n42\nok\n(42,ok)\n"
        );
        let bad = dir.join("Bad.scala");
        for code in [
            "object Bad { val f: Int => Int = x => x; val pf: PartialFunction[Int, Int] = f }",
            "object Bad { val pf: PartialFunction[String, Int] = (x: Int) => x }",
        ] {
            fs::write(&bad, code).unwrap();
            let ok = if ours {
                compile_against(&out, &jar, &bad, &jar).0
            } else {
                Command::new(&scalac)
                    .arg("-d")
                    .arg(&out)
                    .arg(&bad)
                    .output()
                    .unwrap()
                    .status
                    .success()
            };
            assert!(!ok, "ours={ours} accepted {code}");
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn refined_inherited_alias_keeps_receiver_and_method_parameters() {
    source_case(
        "refined-inherited-alias",
        r#"
trait V { type R; type Res = List[R]; def result: Res }
object V { type Aux[A] = V { type R = A } }
object Main {
  def make[A](a: A): V.Aux[A] = new V { type R = A; def result: Res = List(a) }
  def wrap[A](v: V.Aux[A]): V.Aux[Option[v.Res]] =
    new V { type R = Option[v.Res]; def result: Res = List(Some(v.result)) }
  def main(args: Array[String]): Unit = {
    val ints: List[Option[List[Int]]] = wrap(make(42)).result
    println(ints.head.get.head)
  }
}
"#,
        true,
        "42\n",
    );
    source_case(
        "refined-distinct-paths",
        "trait V { type R; type Res = List[R] }; object Main { def bad(p: V, q: V)(x: p.Res): q.Res = x }",
        false,
        "",
    );
}

#[test]
fn binary_abstract_value_uses_refinement_alias() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("abstract-value-refinement");
    let provider = dir.join("provider");
    fs::create_dir_all(&provider).unwrap();
    let libsrc = dir.join("Witness.scala");
    fs::write(&libsrc, "package alib; trait Witness { type T; val value: T }; object Witness { type Aux[A] = Witness { type T = A }; def apply[A](a: A): Aux[A] = new Witness { type T = A; val value: T = a } }; trait ToTraversable[L, M[_], E] { type Out = M[E]; def apply(l: L): Out }; trait ToList[L, E] extends ToTraversable[L, List, E]").unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&provider)
        .arg(&libsrc)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (label, source, expected) in [
        (
            "valid",
            r#"object Main {
          def value[A](w: alib.Witness.Aux[A]): A = w.value
          def integer[A <: Int](w: alib.Witness.Aux[A]): Int = w.value.toInt
          def main(args: Array[String]): Unit = { println(value(alib.Witness("ok"))); println(integer(alib.Witness(42))) }
        }"#,
            Some("ok\n42\n"),
        ),
        (
            "wrong-result",
            "object Main { def bad(w: alib.Witness.Aux[String]): Int = w.value }",
            None,
        ),
        (
            "inherited-constructor-alias",
            r#"object Main {
              def names[K](k: K)(implicit tl: alib.ToList[K, Symbol]): List[String] = tl(k).map(_.name)
              def main(args: Array[String]): Unit = println(names(())(new alib.ToList[Unit, Symbol] {
                def apply(k: Unit): Out = List(Symbol("ok"))
              }).head)
            }"#,
            Some("ok\n"),
        ),
        (
            "inherited-constructor-alias-bad",
            "object Main { def bad[K](k: K)(implicit tl: alib.ToList[K, Symbol]): List[Int] = tl(k).map(_.name) }",
            None,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &provider)
            } else {
                let result = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&provider)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    result.status.success(),
                    String::from_utf8_lossy(&result.stderr).into_owned(),
                )
            };
            assert_eq!(ok, expected.is_some(), "{label}, ours={ours}: {diagnostic}");
            if let Some(expected) = expected {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), provider.display())),
                    expected
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn expected_refinement_constrains_method_result() {
    source_case(
        "expected-refinement",
        r#"
trait V[T, P] { type R; def value: R }
object V {
  type Aux[T, P, R0] = V[T, P] { type R = R0 }
  def instance[T, P, R0](f: () => R0): Aux[T, P, R0] =
    new V[T, P] { type R = R0; def value: R = f() }
  def constant[T, P, R](v: R): Aux[T, P, R] = instance(() => v)
}
object Main { def main(args: Array[String]): Unit = println(V.constant[Int, Unit, String]("ok").value) }
"#,
        true,
        "ok\n",
    );
    source_case(
        "expected-refinement-bad",
        "trait V { type R }; object Main { def instance[A](v: A): V { type R = A } = new V { type R = A }; val bad: V { type R = Int } = instance(\"wrong\") }",
        false,
        "",
    );
}

#[test]
fn binary_constructor_for_prelude_class_has_repeated_parameters() {
    source_case(
        "regex-constructor",
        "object Main { def main(args: Array[String]): Unit = { val r = new scala.util.matching.Regex(\"a+\"); println(r.findFirstIn(\"baab\").get) } }",
        true,
        "aa\n",
    );
    source_case(
        "regex-constructor-bad",
        "object Main { val r = new scala.util.matching.Regex(42) }",
        false,
        "",
    );
}

#[test]
fn concrete_refinement_implicit_beats_polymorphic_fallback() {
    source_case(
        "specific-refinement-implicit",
        r#"
trait W { type T; val value: T }
object W {
  type Aux[A] = W { type T = A }
  implicit def generic[A]: Aux[A] = throw new IllegalStateException("generic chosen")
  implicit val intW: Aux[Int] = new W { type T = Int; val value: T = 42 }
}

object Main { def main(args: Array[String]): Unit = println(implicitly[W.Aux[Int]].value) }
"#,
        true,
        "42\n",
    );
    source_case(
        "ambiguous-refinement-implicit",
        "trait W { type T }; object W { type Aux[A] = W { type T = A }; implicit def a[A]: Aux[A] = ???; implicit def b[A]: Aux[A] = ??? }; object Main { val bad = implicitly[W.Aux[Int]] }",
        false,
        "",
    );
}

#[test]
fn implicit_type_constraints_keep_literal_singletons() {
    source_case(
        "literal-implicit",
        r#"
trait Evidence[A] { def label: String }
object Evidence { implicit def evidence[A]: Evidence[A] = new Evidence[A] { def label: String = "ok" } }
trait Dependent { type T; def label: String }
object Dependent { type Aux[A] = Dependent { type T = A }; implicit def evidence[A]: Aux[A] = new Dependent { type T = A; def label: String = "dependent" } }
object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[Evidence[1]].label)
    println(implicitly[Dependent.Aux[1]].label)
  }
}
"#,
        true,
        "ok\ndependent\n",
    );
    source_case("literal-implicit-bad", "trait Evidence[A]; object Main { implicit val wrong: Evidence[2] = new Evidence[2] {}; val bad = implicitly[Evidence[1]] }", false, "");
}

#[test]
fn fully_qualified_java_static_methods_eta_expand_without_receiver() {
    source_case(
        "qualified-static-eta",
        r#"
object Main {
  def call[T](f: (T,T) => Int, a:T, b:T):Int = f(a,b)
  def main(args:Array[String]):Unit = {
    println(call(java.lang.Double.compare, 1.0, 2.0))
    println(call(java.lang.Float.compare, 2.0f, 1.0f))
  }
}
"#,
        true,
        "-1\n1\n",
    );
}

#[test]
fn primitive_extension_prefers_float_over_widening_to_double() {
    source_case(
        "float-isnan",
        "object Main { def nan(x: Float): Boolean = x.isNaN; def main(args: Array[String]): Unit = { println(nan(Float.NaN)); println(nan(1.0f)) } }",
        true,
        "true\nfalse\n",
    );
}

#[test]
fn binary_projection_alias_preserves_widened_prefix() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("projection-prefix");
    let lib = build_lib_jar(&dir);
    for (label, result, accepted) in [
        ("projected", "alib.ProjectionProfile.api.Result", true),
        ("stable", "alib.ProjectionBackend.Result", false),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
object Main {{
  def convert(value: alib.ProjectionProfile#Backend#Result): {result} = value
  def main(args: Array[String]): Unit = println(convert(new alib.ProjectionBackend.Entry) != null)
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "true\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn generic_map_lambda_result_selects_scalar_or_pair_overload() {
    source_case(
        "generic-map-result",
        r#"
case class Row[A](value: A)
case class Rows[A](values: Iterable[Row[A]])
object Main {
  def rows[A](values: Seq[A]): Rows[A] =
    Rows(for { (_, group) <- values.groupBy(_ => 0) } yield Row(group.head))
  def pairs[A](values: Seq[A]): Map[Int, Row[A]] =
    for { (key, group) <- values.groupBy(_ => 0) } yield (key, Row(group.head))
  def main(args: Array[String]): Unit = {
    println(rows(Seq(42)).values.head.value)
    println(pairs(Seq("ok"))(0).value)
  }
}
"#,
        true,
        "42\nok\n",
    );
    source_case(
        "generic-map-result-bad",
        r#"
case class Row[A](value: A)
object Main {
  def wrong[A](values: Map[Int,A]): Map[Int,Row[A]] =
    for { (_, value) <- values } yield Row(value)
}
"#,
        false,
        "",
    );
}

#[test]
fn binary_alias_keeps_static_singleton_receiver() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("singleton-alias-receiver");
    let provider = dir.join("provider");
    fs::create_dir_all(&provider).unwrap();
    let libsrc = dir.join("Library.scala");
    fs::write(
        &libsrc,
        r#"
package alib
trait Tagged[Tag, A] { def result(value: A): String }
trait Operation { type Impl[A] = Tagged[this.type, A] }
object Update extends Operation
object Read extends Operation
object Container {
  def combine[A](value: A)(implicit ev: Update.Impl[A]): String = ev.result(value)
}
"#,
    )
    .unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&provider)
        .arg(&libsrc)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (label, tag, expected) in [
        ("right", "Update", Some("value=42\n")),
        ("other-operation", "Read", None),
        ("unrelated-receiver", "Container", None),
        ("inherited-constructor", "nested", Some("value=42\n")),
        ("distinct-constructor", "nested-other", None),
    ] {
        let src = dir.join(format!("{label}.scala"));
        let source = if tag.starts_with("nested") {
            let witness = if tag == "nested" { "Main" } else { "Other" };
            format!(
                r#"
import alib._
object Other extends Operation {{
  implicit object evidence extends Impl[Int] {{ def result(value: Int): String = "other" }}
}}
object Main extends Operation {{
  implicit object evidence extends Impl[Int] {{ def result(value: Int): String = "value=" + value }}
  val selected: Tagged[Main.type, Int] = {witness}.evidence
  def main(args: Array[String]): Unit = println(selected.result(42))
}}
"#
            )
        } else {
            format!(
                r#"
import alib._
object Main {{
  implicit val evidence: Tagged[{tag}.type, Int] = new Tagged[{tag}.type, Int] {{
    def result(value: Int): String = "value=" + value
  }}
  def main(args: Array[String]): Unit = println(Container.combine(42))
}}
"#
            )
        };
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &provider)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&provider)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, expected.is_some(), "{label}, ours={ours}: {diagnostic}");
            if let Some(expected) = expected {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), provider.display())),
                    expected
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn inherited_binary_alias_and_inferred_assignment_operator_are_completed() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("binary-alias-operator");
    let lib = build_lib_jar(&dir);
    for (label, source, expected) in [
        (
            "alias",
            r#"object Main {
          def read(row: alib.ConcreteResultTable#Element): Int = row.value
          def main(args: Array[String]): Unit = println(read(alib.ResultRow(42)))
        }"#,
            Some("42\n"),
        ),
        (
            "operator",
            r#"object Main {
          def main(args: Array[String]): Unit = println(alib.ResultExpression { context =>
            context.builder += "ok"
            context.builder += "!"
          })
        }"#,
            Some("ok!\n"),
        ),
        (
            "alias-bad",
            "object Main { def bad(row: alib.ConcreteResultTable#Element): String = row.value }",
            None,
        ),
        (
            "operator-bad",
            "object Main { val bad = alib.ResultExpression { context => context.builder += 42 } }",
            None,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, expected.is_some(), "{label}, ours={ours}: {diagnostic}");
            if let Some(expected) = expected {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    expected
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn lambda_result_inference_keeps_receiver_lower_bound_without_expected_type() {
    source_case(
        "unannotated-recover",
        r#"
import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.duration.Duration
case class Item(value: Int)
object Main {
  implicit val ec: ExecutionContext = ExecutionContext.global
  def recovered(input: Future[Option[Item]]): Future[Option[Int]] = {
    val result = input.recover { case _: IllegalArgumentException => None }
    result.map(_.map(_.value))
  }
  def main(args: Array[String]): Unit = {
    println(Await.result(recovered(Future.successful(Some(Item(42)))), Duration(10, "seconds")))
    println(Await.result(recovered(Future.failed(new IllegalArgumentException)), Duration(10, "seconds")))
  }
}
"#,
        true,
        "Some(42)\nNone\n",
    );
    source_case(
        "unannotated-recover-bad",
        r#"
import scala.concurrent.{ExecutionContext, Future}
object Main {
  def recovered(input: Future[Option[Int]])(implicit ec: ExecutionContext): Future[None.type] = {
    val result = input.recover { case _: IllegalArgumentException => None }
    result
  }
}
"#,
        false,
        "",
    );
}

#[test]
fn implicit_arguments_infer_open_factory_receiver_parameters() {
    source_case(
        "factory-receiver-evidence",
        r#"
trait Context[F[_]] { def label: String }
object Context {
  implicit val list: Context[List] = new Context[List] { def label: String = "list" }
}
final class Wrapped[F[_], A](val value: A, val label: String) {
  def map[B](f: A => B)(implicit ctx: Context[F]): Wrapped[F, B] = new Wrapped(f(value), ctx.label)
}
object Wrapped {
  def make[F[_]] = new Maker[F]
  final class Maker[F[_]] {
    def apply[A](value: A)(implicit ctx: Context[F]): Wrapped[F, A] = new Wrapped(value, ctx.label)
  }
}
object Main {
  def result: Wrapped[List, Int] = { val n = 41; Wrapped.make(n) }.map(_ + 1)
  def main(args: Array[String]): Unit = {
    println(result.value)
    println(result.label)
  }
}
"#,
        true,
        "42\nlist\n",
    );
    source_case(
        "factory-receiver-rigid",
        r#"
trait Context[F[_]]
object Context { implicit val list: Context[List] = new Context[List] {} }
object Factory {
  def make[F[_]] = new Maker[F]
  class Maker[F[_]] { def apply[A](a: A)(implicit ctx: Context[F]): A = a }
}
object Main { def rigid[F[_]]: Int = Factory.make[F](1) }
"#,
        false,
        "",
    );
}

#[test]
fn member_value_projection_and_cold_package_extractor_remain_available() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("member-value-projection");
    let provider = dir.join("provider");
    fs::create_dir_all(&provider).unwrap();
    let libsrc = dir.join("Library.scala");
    fs::write(
        &libsrc,
        r#"
package alib
trait FactoryApi { def result: Int }
trait Backend { type Factory <: FactoryApi; val factory: Factory }
class ConcreteBackend extends Backend {
  class Factory extends FactoryApi { def result: Int = 42 }
  val factory = new Factory
}
trait Profile {
  type Engine <: Backend
  val backend: Engine
  trait API { val factory: backend.Factory = backend.factory }
  val api: API
}
object Profile extends Profile {
  type Engine = ConcreteBackend
  val backend = new ConcreteBackend
  object api extends API
}
"#,
    )
    .unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&provider)
        .arg(&libsrc)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (label, source, expected) in [
        (
            "member-value",
            r#"object Main {
          val factory = alib.Profile.api.factory
          def main(args: Array[String]): Unit = println(factory.result)
        }"#,
            Some("42\n"),
        ),
        (
            "member-value-bad",
            "object Main { val bad: String = alib.Profile.api.factory.result }",
            None,
        ),
        (
            "cold-extractor",
            r#"object Main {
          def head(xs: List[Int]): Int = xs match { case x :: rest => x; case Nil => 0 }
          def main(args: Array[String]): Unit = println(head(List(42)))
        }"#,
            Some("42\n"),
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &provider)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&provider)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, expected.is_some(), "{label}, ours={ours}: {diagnostic}");
            if let Some(expected) = expected {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), provider.display())),
                    expected
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn implicit_evidence_partially_unifies_higher_kinded_arguments() {
    source_case(
        "evidence-partial-unification",
        r#"
object Main {
  def infer[A, F[_], B](value: A)(implicit ev: A <:< F[B]): F[B] = ev(value)
  val input: Either[String, Int] = Right(42)
  val result = infer(input)
  val check: Either[String, Int] = result
  def main(args: Array[String]): Unit = println(result.toOption.get)
}
"#,
        true,
        "42\n",
    );
    source_case(
        "evidence-partial-unification-bad",
        r#"
object Main {
  def infer[A, F[_], B](value: A)(implicit ev: A <:< F[B]): F[B] = ev(value)
  val input: Either[String, Int] = Right(42)
  val result = infer(input)
  val bad: Either[Int, Int] = result
}
"#,
        false,
        "",
    );
    source_case(
        "evidence-constructor-arity-bad",
        r#"
object Main {
  def infer[A, F[_, _], B, C](value: A)(implicit ev: A <:< F[B, C]): F[B, C] = ev(value)
  val input: Option[Int] = Some(42)
  val bad = infer(input)
}
"#,
        false,
        "",
    );
}

#[test]
fn binary_nullary_getter_applies_its_projected_class_result() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("projected-callable-getter");
    let lib = build_lib_jar(&dir);
    for (label, expression, accepted) in [
        ("apply", "Submit(\"path\", 42)", true),
        (
            "explicit-empty",
            "alib.CallableBuilders.fresh()(\"path\", 42)",
            true,
        ),
        ("missing-evidence", "Submit(\"path\", \"bad\")", false),
        (
            "missing-empty-clause",
            "alib.CallableBuilders.fresh(\"path\", 42)",
            false,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
import alib.CallableBuilders.Submit
object Main {{
  implicit val writer:alib.Writer[Int] = new alib.Writer[Int] {{ def write(value:Int):Int = value }}
  val result:Int = {expression}
  def main(args:Array[String]):Unit = println(result)
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "42\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn tuple_conversion_partially_unifies_nested_higher_kinded_arguments() {
    let source = r#"
object Main {
  class Ops[F[_], A, B](pair: (F[A], F[B])) { def first: F[A] = pair._1 }
  implicit def pairOps[F[_], A, B](pair: (F[A], F[B])): Ops[F, A, B] = new Ops(pair)
  val a: Either[String, Int] = Right(42)
  val b: Either[String, Boolean] = Right(true)
  val first = (a, b).first
  val check: Either[String, Int] = first
  def main(args: Array[String]): Unit = println(first.toOption.get)
}
"#;
    source_case(
        "nested-conversion-partial-unification",
        source,
        true,
        "42\n",
    );
    source_case(
        "nested-conversion-partial-unification-bad",
        &source.replace(
            "val check: Either[String, Int]",
            "val check: Either[Int, Int]",
        ),
        false,
        "",
    );
}

#[test]
fn binary_abstract_alias_keeps_its_module_implicit_scope() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("abstract-alias-module-scope");
    let lib = build_lib_jar(&dir);
    for (label, result_type, accepted) in
        [("valid", "Int", true), ("wrong-result", "String", false)]
    {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
import alib.EncodedAliases.Value
object Main {{
  def unwrap(value: Value[Int]): {result_type} = value.underlying
  def main(args: Array[String]): Unit = println(unwrap(alib.EncodedValue.create(42)))
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "42\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn binary_operator_result_preserves_its_parameter_path() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("operator-dependent-result");
    let lib = build_lib_jar(&dir);
    for (result_type, accepted) in [("Int", true), ("String", false)] {
        let src = dir.join(format!("{result_type}.scala"));
        fs::write(
            &src,
            format!(
                r#"
import alib.{{DependentOperator, OperatorResult}}
object Main {{
  implicit val result:OperatorResult {{type Out=Int}} =
    new OperatorResult {{type Out=Int; def value:Int=42}}
  val value:{result_type} = DependentOperator / 1
  def main(args:Array[String]):Unit = println(value)
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{result_type}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "42\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn binary_conversion_loads_nested_argument_companions() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let Some(scalac) = scalac() else { return };
    let dir = tmp_dir("nested-argument-companion");
    let lib = build_lib_jar(&dir);
    for (label, expression, accepted) in [
        ("argument", "alib.NestedViewReceiver.accept(value)", true),
        (
            "expected-type",
            "{ val target: alib.NestedViewTarget = value; target.value }",
            true,
        ),
        (
            "wrong-function",
            "alib.NestedViewReceiver.accept((x: String) => Option.empty[alib.NestedViewResult])",
            false,
        ),
    ] {
        let src = dir.join(format!("{label}.scala"));
        fs::write(
            &src,
            format!(
                r#"
object Main {{
  def convert(value: Int => Option[alib.NestedViewResult]): Int = {expression}
  def main(args: Array[String]): Unit = println(convert(_ => None))
}}
"#
            ),
        )
        .unwrap();
        for ours in [false, true] {
            let out = dir.join(format!("{label}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let (ok, diagnostic) = if ours {
                compile_against(&out, &jar, &src, &lib)
            } else {
                let output = Command::new(&scalac)
                    .arg("-cp")
                    .arg(&lib)
                    .arg("-d")
                    .arg(&out)
                    .arg(&src)
                    .output()
                    .unwrap();
                (
                    output.status.success(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            };
            assert_eq!(ok, accepted, "{label}, ours={ours}: {diagnostic}");
            if accepted {
                assert_eq!(
                    run_java(&out, &format!("{}:{}", jar.display(), lib.display())),
                    "42\n"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn collection_concat_matches_scalac() {
    let source = r#"
import scala.collection.immutable.ArraySeq

object Main {
  val seq = Seq(1).concat(",")
  val vector = Vector(2L).concat(",")
  val iterable = Iterable(3.0).concat(",")
  val indexed = IndexedSeq(4.toShort).concat(",")
  val list = List(5.toByte).concat(",")
  val arraySeq = ArraySeq('x').concat(",")
  val companion = Seq.concat(Seq(7), Seq(8L))
  val string = "a".concat("b")

  def main(args: Array[String]): Unit = {
    println(seq.mkString("|"))
    println(vector.mkString("|"))
    println(iterable.mkString("|"))
    println(indexed.mkString("|"))
    println(list.mkString("|"))
    println(arraySeq.mkString("|"))
    println(companion.mkString("|"))
    println(string)
  }
}
"#;
    source_case(
        "collection-concat",
        source,
        true,
        "1|,\n2|,\n3.0|,\n4|,\n5|,\nx|,\n7|8\nab\n",
    );
}
