//! Two Java-interop gaps, and the two cascades behind the first one.
//!
//!  * A **nested** Java generic interface lost its type parameters.
//!    `java/util/Map$Entry` reaches the symbol table as a *stub* long before
//!    anyone writes its name — `Map.entrySet()`'s generic signature mentions
//!    it — and `complete_binary_member` returned on the strength of that stub,
//!    so `java/util/Map$Entry.class`, the file that carries the nested
//!    `Signature` (`<K:…;V:…>`), was never read. `java.util.Map.Entry[String,
//!    Int]` was "Entry does not take type parameters".
//!
//!    Two cascades hung off it, both reproducible on their own:
//!    - SLS 5.1.2 resolves a class reachable through two parents in favour of
//!      the *later* list; `c3_merge`'s fallback emitted it twice, the first
//!      time too early. Java re-`implements` an interface its own superclass
//!      already implements all the time (`class LinkedHashMap extends HashMap
//!      implements Map`), so `java.util.Map` landed *before* `HashMap` and
//!      `HashMap.put` stopped counting as an implementation of `Map.put`.
//!    - A Java interface re-declares `equals` / `hashCode` (JLS 9.2) and
//!      re-declares its superinterfaces' methods (`java.util.List` over
//!      `java.util.Collection`). `java.lang.Object` and the superclass chain
//!      implement those wherever they sit in the linearization.
//!
//!  * A `static` method of an *interface* went out as a
//!    `CONSTANT_Methodref`. JVMS 4.4.2 wants `CONSTANT_InterfaceMethodref`;
//!    `invokestatic` itself was already right, so this type checked and then
//!    threw `IncompatibleClassChangeError` at the first call — a silent
//!    miscompile of every Java 9+ interface factory (`Map.entry`, `List.of`,
//!    `Map.of`, …).
//!
//! Plus the miscompile the LRU-cache probe hit once it compiled: the typer
//! wraps an erased generic result in `$unbox`, and in statement position nsc
//! drops the value instead of adapting it. `m.put("a", 1)` returns the
//! *previous* value, so unboxing only to `pop` threw `NullPointerException` at
//! the first insert.
//!
//! Fixtures are dual-run: against the real `scala-library` jar and, where the
//! private runtime can back them, on it — under `-Xverify:all`, with the
//! stdout nsc 2.13.16 prints.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-jnest-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(out: &Path, jar: Option<&Path>, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    match jar {
        Some(j) => cmd.args(["--scala-library", j.to_str().unwrap()]),
        None => cmd.arg("--no-scala-library"),
    };
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile and run a fixture in both modes and compare with nsc's stdout.
fn dual_run(name: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let exp = expected(name);

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src));
    assert!(ok, "compile {name} (private runtime) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&priv_out, None),
            exp,
            "stdout mismatch for {name} on the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&priv_out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} (jar): scala-library jar not present");
        return;
    };
    let jar_out = tmp_dir("jar");
    let (ok, msgs) = compile(&jar_out, Some(&jar), &[src]);
    assert!(ok, "compile {name} (jar) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&jar_out, Some(&jar)),
            exp,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
}

fn accepts(tag: &str, source: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn rejects(tag: &str, source: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "{tag} should not compile, got:\n{msgs}");
    for n in needles {
        assert!(
            msgs.contains(n),
            "expected {n:?} in diagnostics for {tag}, got {msgs:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- (1)

/// `java.util.Map.Entry[K, V]`: the nested interface's own type parameters,
/// a Scala class implementing it, a wildcard application, and a nested class
/// two levels down.
#[test]
fn jn_nested_java_interface_keeps_its_type_parameters() {
    dual_run("jn_nested");
}

/// The raw nested type still takes no arguments, exactly as in Java.
#[test]
fn jn_raw_nested_type_is_accepted() {
    accepts(
        "jn_raw",
        r#"
object Main {
  def key(e: java.util.Map.Entry[_, _]): Any = e.getKey
  def raw(m: java.util.Map[String, Int]): Int = m.size()
  def main(args: Array[String]): Unit = println(raw(new java.util.HashMap[String, Int]()))
}
"#,
    );
}

/// Applying a nested type constructor to the wrong number of arguments is
/// still an error — the fix must not make `Entry` take anything.
#[test]
fn jn_nested_arity_is_still_checked() {
    rejects(
        "jn_arity",
        r#"
object Main {
  def f(e: java.util.Map.Entry[String, Int, Long]): Any = e.getKey
  def main(args: Array[String]): Unit = ()
}
"#,
        &["too many type arguments"],
    );
}

/// Extending a Java class whose *superclass chain* implements the interfaces
/// it re-declares. Each of these was "needs to be abstract" before.
#[test]
fn jn_extending_java_collections_is_concrete() {
    accepts(
        "jn_extend",
        r#"
class C1 extends java.util.HashMap[String, Int]
class C2 extends java.util.ArrayList[String]
class C3 extends java.util.LinkedHashMap[String, Int]
class C4 extends java.util.LinkedList[String]
class C5 extends java.lang.Thread
abstract class A1 extends java.util.HashMap[String, Int]
class C6 extends A1
object Main { def main(args: Array[String]): Unit = println(new C1().size()) }
"#,
    );
}

/// `java.lang.Object`'s concrete members implement what a trait re-declares
/// deferred, the same way they do for a Java interface.
#[test]
fn jn_object_members_implement_deferred_declarations() {
    accepts(
        "jn_objmem",
        r#"
trait T1 { def hashCode(): Int }
trait T2 { def equals(o: Any): Boolean }
trait T3 { def toString(): String }
class D1 extends T1
class D2 extends T2
class D3 extends T3
object Main { def main(args: Array[String]): Unit = println(new D1().hashCode() != 0) }
"#,
    );
}

/// …but a member nothing implements is still missing, and `equals` /
/// `hashCode` must not be listed alongside it.
#[test]
fn jn_missing_nested_interface_members_still_reported() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip jn_nested_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("jn_nested_bad.scala");
    let out = tmp_dir("jn_nested_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "jn_nested_bad should not compile, got:\n{msgs}");
    assert!(
        msgs.contains("class Half needs to be abstract."),
        "got:\n{msgs}"
    );
    for n in ["getValue", "setValue"] {
        assert!(msgs.contains(n), "expected {n} in:\n{msgs}");
    }
    for n in ["equals", "hashCode", "getKey"] {
        assert!(!msgs.contains(n), "{n} must not be listed in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------------- (2)

/// Interface statics (`CONSTANT_InterfaceMethodref`), interface default
/// methods (`invokeinterface`) and class statics (`CONSTANT_Methodref`), all
/// from the real JDK, run under `-Xverify:all`.
#[test]
fn jn_interface_statics_use_interface_methodref() {
    dual_run("jn_static");
}

/// The constant pool itself, so a passing run cannot hide a wrong tag.
#[test]
fn jn_interface_static_constant_has_the_interface_tag() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip jn_pool: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("jn_pool");
    let src = dir.join("jn_pool.scala");
    fs::write(
        &src,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(java.util.Map.entry("k", 7).getKey)
    println(java.util.List.of("a", "b", "c").size())
    println(java.lang.Integer.valueOf(1))
  }
}
"#,
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(ok, "jn_pool failed:\n{msgs}");
    let bytes = fs::read(out.join("Main$.class")).unwrap();
    // CONSTANT_InterfaceMethodref = 11, CONSTANT_Methodref = 10. Read the
    // whole pool rather than grepping bytes: a `Methodref` for an interface
    // owner is the bug, and it is invisible in the disassembly of the
    // instruction (`invokestatic` either way).
    let pool = constant_pool_member_refs(&bytes);
    let find = |owner: &str, name: &str| {
        pool.iter()
            .find(|(_, o, n, _)| o == owner && n == name)
            .unwrap_or_else(|| panic!("{owner}.{name} not in the pool: {pool:?}"))
            .0
    };
    assert_eq!(find("java/util/Map", "entry"), 11, "pool: {pool:?}");
    assert_eq!(find("java/util/List", "of"), 11, "pool: {pool:?}");
    assert_eq!(find("java/lang/Integer", "valueOf"), 10, "pool: {pool:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// `(tag, owner, name, descriptor)` for every `Fieldref` / `Methodref` /
/// `InterfaceMethodref` in a class file's constant pool.
fn constant_pool_member_refs(bytes: &[u8]) -> Vec<(u8, String, String, String)> {
    let u16at = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
    let count = u16at(8);
    // index -> (tag, payload offset)
    let mut at: Vec<(u8, usize)> = vec![(0, 0); count];
    let mut p = 10;
    let mut i = 1;
    while i < count {
        let tag = bytes[p];
        at[i] = (tag, p + 1);
        p += 1 + match tag {
            1 => 2 + u16at(p + 1),
            7 | 8 | 16 | 19 | 20 => 2,
            15 => 3,
            5 | 6 => {
                i += 1; // longs and doubles take two entries
                8
            }
            _ => 4,
        };
        i += 1;
    }
    let utf8 = |idx: usize| {
        let (tag, off) = at[idx];
        assert_eq!(tag, 1);
        let len = u16::from_be_bytes([bytes[off], bytes[off + 1]]) as usize;
        String::from_utf8_lossy(&bytes[off + 2..off + 2 + len]).into_owned()
    };
    let mut out = Vec::new();
    for idx in 1..count {
        let (tag, off) = at[idx];
        // Fieldref (9), Methodref (10), InterfaceMethodref (11).
        if !matches!(tag, 9..=11) {
            continue;
        }
        let class = u16::from_be_bytes([bytes[off], bytes[off + 1]]) as usize;
        let nat = u16::from_be_bytes([bytes[off + 2], bytes[off + 3]]) as usize;
        let owner = utf8(u16::from_be_bytes([bytes[at[class].1], bytes[at[class].1 + 1]]) as usize);
        let (_, noff) = at[nat];
        let name = utf8(u16::from_be_bytes([bytes[noff], bytes[noff + 1]]) as usize);
        let desc = utf8(u16::from_be_bytes([bytes[noff + 2], bytes[noff + 3]]) as usize);
        out.push((tag, owner, name, desc));
    }
    out
}

// ------------------------------------------------------------- the probe

/// The whole thing the gap was found with: an LRU cache over
/// `java.util.LinkedHashMap` overriding `removeEldestEntry(Map.Entry[K, V])`,
/// a `Thread` subclass, an anonymous `Comparator` and `Arrays.sort`.
#[test]
fn jn_lru_cache_probe_matches_scalac() {
    dual_run("jn_lru");
}

/// A discarded erased generic result is dropped, not unboxed: `put` returns
/// the previous value and the first insert has none.
#[test]
fn jn_discarded_erased_result_is_not_unboxed() {
    accepts(
        "jn_discard",
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val m = new java.util.HashMap[String, Int]()
    m.put("a", 1)
    m.remove("a")
    val l = new java.util.ArrayList[Long]()
    l.add(1L)
    println(m.size() + l.size())
  }
}
"#,
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    if !java_available() {
        return;
    }
    let dir = tmp_dir("jn_discard_run");
    let src = dir.join("Main.scala");
    fs::write(
        &src,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val m = new java.util.HashMap[String, Int]()
    m.put("a", 1)
    m.remove("a")
    m.put("b", 2)
    val l = new java.util.ArrayList[Long]()
    l.add(1L)
    l.set(0, 2L)
    println(m.size() + " " + l.get(0))
  }
}
"#,
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(ok, "jn_discard_run failed:\n{msgs}");
    assert_eq!(run_main(&out, Some(&jar)), "1 2\n");
    let _ = fs::remove_dir_all(&dir);
}
