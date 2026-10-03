//! Java annotations written on Scala definitions reach the class file where
//! scalac puts them, so reflection-driven libraries (JAX-RS, OpenAPI, JSON
//! mappers) see the same thing on either compiler's output.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::path::Path;
use std::{fs, process::Command};

const RT: &str = r#"
package ja;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME)
public @interface Rt {
  String value() default "";
  long l() default 0;
  char c() default 'a';
  double d() default 0;
  float f() default 0;
  boolean b() default false;
  byte by() default 0;
  short sh() default 0;
  int[] is() default {};
  Class<?>[] cs() default {};
  ElementType e() default ElementType.TYPE;
  Nested n() default @Nested;
  Nested[] ns() default {};
}
"#;

const NESTED: &str = r#"
package ja;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME)
public @interface Nested { String value() default ""; Class<?> k() default Object.class; }
"#;

const REP: &str = r#"
package ja;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME)
@Repeatable(Reps.class)
public @interface Rep { String value(); }
"#;

const REPS: &str = r#"
package ja;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME)
public @interface Reps { Rep[] value(); }
"#;

const CLS: &str = r#"
package ja;
import java.lang.annotation.*;
@Retention(RetentionPolicy.CLASS)
public @interface Cls {}
"#;

const SRC: &str = r#"
package ja;
import java.lang.annotation.*;
@Retention(RetentionPolicy.SOURCE)
public @interface Src {}
"#;

/// Prints every annotation reflection finds on the named classes, their
/// members and their parameters, in an order independent of the class
/// file's member order.
const DUMP: &str = r#"
package ja;
import java.lang.reflect.*;
import java.util.*;
public class Dump {
  // `Annotation.toString` orders the elements through a `HashMap` of the
  // defaults, and so by `getDeclaredMethods`, whose order differs between JVM
  // runs: elements are printed sorted by name instead.
  static String show(Object v) throws Exception {
    if (v instanceof java.lang.annotation.Annotation) return annot((java.lang.annotation.Annotation) v);
    if (v instanceof Object[]) {
      List<String> out = new ArrayList<>();
      for (Object o : (Object[]) v) out.add(show(o));
      return out.toString();
    }
    if (v != null && v.getClass().isArray()) {
      List<String> out = new ArrayList<>();
      for (int i = 0; i < java.lang.reflect.Array.getLength(v); i++) out.add(String.valueOf(java.lang.reflect.Array.get(v, i)));
      return out.toString();
    }
    if (v instanceof Class) return ((Class<?>) v).getName();
    return String.valueOf(v);
  }
  static String annot(java.lang.annotation.Annotation a) throws Exception {
    Method[] ms = a.annotationType().getDeclaredMethods();
    Arrays.sort(ms, Comparator.comparing(Method::getName));
    List<String> out = new ArrayList<>();
    for (Method m : ms) out.add(m.getName() + "=" + show(m.invoke(a)));
    return "@" + a.annotationType().getName() + out;
  }
  static String annots(java.lang.annotation.Annotation[] as) throws Exception {
    List<String> out = new ArrayList<>();
    for (java.lang.annotation.Annotation a : as)
      if (!a.annotationType().getName().startsWith("scala.reflect.Scala")) out.add(annot(a));
    return out.toString();
  }
  static String params(java.lang.annotation.Annotation[][] ps) throws Exception {
    StringBuilder b = new StringBuilder();
    for (java.lang.annotation.Annotation[] p : ps) b.append(annots(p));
    return b.toString();
  }
  // Only members that carry an annotation: the member lists of the two
  // compilers' output differ in ways unrelated to annotations.
  static void add(List<String> lines, String member, String own, String params) {
    if (!own.equals("[]") || params.contains("@")) lines.add(member + " " + own + " " + params);
  }
  public static void main(String[] args) throws Exception {
    for (String name : args) {
      Class<?> c = Class.forName(name);
      List<String> lines = new ArrayList<>();
      for (Field f : c.getDeclaredFields())
        add(lines, "field " + f.getName(), annots(f.getDeclaredAnnotations()), "");
      for (Method m : c.getDeclaredMethods())
        add(lines, "method " + m.getName() + Arrays.toString(m.getParameterTypes())
          + (m.isBridge() ? " bridge" : ""), annots(m.getDeclaredAnnotations()),
          params(m.getParameterAnnotations()));
      for (Constructor<?> k : c.getDeclaredConstructors())
        add(lines, "ctor " + Arrays.toString(k.getParameterTypes()), "[]",
          params(k.getParameterAnnotations()));
      Collections.sort(lines);
      System.out.println("class " + name + " " + annots(c.getDeclaredAnnotations()));
      for (String l : lines) System.out.println("  " + l);
    }
  }
}
"#;

const SOURCE: &str = r#"
package t
import ja._
import scala.annotation.meta._

object Consts { final val N = "n"; final val I = 7 }

@Rt(value = "k" + Consts.N, l = -3L, c = 'z', d = 1.5, f = 2, b = true, by = 1, sh = -2,
  is = Array(1, Consts.I), cs = Array(classOf[String], classOf[Int], classOf[Array[String]], classOf[Unit]),
  e = java.lang.annotation.ElementType.METHOD, n = new Nested("in"),
  ns = Array(new Nested(value = "a", k = classOf[K]), new Nested("b")))
@Cls @Src
class K(@(Rt @field)("kf") val a: Int, @(Rt @getter)("kg") val b: Int,
    @(Rt @param @field)("both") val c: Int, @Rt("plain") d: Int, @Rt("pv") val e0: String) {
  @(Rt @getter @setter)("gs") var e = 1
  @(Rt @field @getter)("fg") val f = 2
  @Rt("body") val g = 3
  @Rt("var") var h = 4
  def this() = this(1, 2, 3, 4, "")
  @Rt("lazy") lazy val lz = 5
  @Rt("m") @Cls def m(@Rt("p1") x: Int, y: String, @Rt("p3") @Nested z: Long): Int = x + d
  @Deprecated def old(): Unit = ()
  @deprecated("m", "1") def old2(): Unit = ()
  @Rep("r1") @Rt("between") @Rep("r2") def repeated(): Unit = ()
  @Rep("once") def single(): Unit = ()
}

@Rt("obj") object O { @Rt("om") def m(@Rt("omp") x: Int) = x; @Rt("ov") val v = 2 }

@Rt("tr") trait T { @Rt("tc") def c(@Rt("tp") x: Int): Int = x; @Rt("ta") def a(@Rt("tap") y: Int): Int; @Rt("tv") val tv: Int = 3 }

case class CC(@Rt("cc") x: Int, @(Rt @field)("ccf") y: Int)

trait Mixed { @Rt("mixed") protected val mv: Int = 1; @Rt("mixedVar") var mw: String = "" }
class Impl extends Mixed

abstract class Base[A] { def g(a: A): A }
class G extends Base[String] { @Rt("g") def g(@Rt("ga") a: String): String = a; @Rt("poly") def p[B](b: B): B = b }
"#;

const CLASSES: &[&str] = &[
    "t.K", "t.O", "t.O$", "t.T", "t.CC", "t.G", "t.Mixed", "t.Impl",
];

fn javac(javac: &Path, out: &Path, sources: &[&Path]) {
    let output = Command::new(javac)
        .arg("-d")
        .arg(out)
        .args(sources)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dump(java: &Path, cp: &str) -> String {
    let output = Command::new(java)
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(cp)
        .arg("ja.Dump")
        .args(CLASSES)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "reflection dump failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Whether `class` holds `needle` anywhere, as a constant-pool string.
fn mentions(class: &Path, needle: &str) -> bool {
    let bytes = fs::read(class).unwrap();
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn java_annotations_match_scalac_placement() {
    let t = toolchain();
    let (Some(library), Some(javac_bin), Some(java), Some(scalac)) =
        (t.scala_library(), t.javac(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, javac, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("java-annotations");
    let jsrc = dir.join("ja");
    let jout = dir.join("jout");
    fs::create_dir_all(&jsrc).unwrap();
    fs::create_dir_all(&jout).unwrap();
    let mut javas = Vec::new();
    for (name, text) in [
        ("Rt", RT),
        ("Nested", NESTED),
        ("Rep", REP),
        ("Reps", REPS),
        ("Cls", CLS),
        ("Src", SRC),
        ("Dump", DUMP),
    ] {
        let p = jsrc.join(format!("{name}.java"));
        fs::write(&p, text).unwrap();
        javas.push(p);
    }
    let refs: Vec<&Path> = javas.iter().map(|p| p.as_path()).collect();
    javac(javac_bin, &jout, &refs);

    let source = dir.join("K.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    let cp = format!("{}:{}", jout.display(), library.display());
    CompileCommand::new(&source, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-classpath")
        .arg(&cp)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scalac failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = dump(java, &format!("{cp}:{}", theirs.display()));
    let actual = dump(java, &format!("{cp}:{}", ours.display()));
    assert!(
        expected.contains("@ja.Rt[")
            && expected.contains("value=kf")
            && expected.contains("@ja.Reps["),
        "the scalac dump must show annotations:\n{expected}"
    );
    let diff: Vec<String> = expected
        .lines()
        .zip(actual.lines())
        .filter(|(e, a)| e != a)
        .map(|(e, a)| format!("scalac:   {e}\nscala-rs: {a}"))
        .collect();
    assert!(
        diff.is_empty() && expected.lines().count() == actual.lines().count(),
        "reflection sees different annotations:\n{}\n--- scalac ---\n{expected}\n--- scala-rs ---\n{actual}",
        diff.join("\n")
    );

    // Reflection cannot see `CLASS` retention; the class file must still
    // carry it, and never a `SOURCE` one.
    for root in [&theirs, &ours] {
        let k = root.join("t/K.class");
        assert!(
            mentions(&k, "RuntimeInvisibleAnnotations"),
            "{}",
            k.display()
        );
        assert!(mentions(&k, "Lja/Cls;"), "{}", k.display());
        assert!(!mentions(&k, "Lja/Src;"), "{}", k.display());
    }
}
