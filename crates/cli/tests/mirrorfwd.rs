//! Static forwarders from a top-level `object` onto its **companion class**.
//!
//! scala-rs only ever emitted them onto a *mirror* class -- the `Main.class`
//! it synthesizes when `object Main` has no companion. When the source wrote
//! `class Main` next to `object Main`, nsc emits no mirror class and puts the
//! same forwarders on the companion's own classfile; scala-rs emitted
//! nothing, so `Main.class` had no `main` and `java Main` could not start the
//! program. That is `scala/scala`'s `run/t363` and eight other tests of its
//! corpus, and it is what `fixtures_mirrorfwd` covers end to end.
//!
//! The second half of the file checks the *selection rule* against what real
//! scalac 2.13.16 was measured to do (`javap -p`, one probe per question, see
//! `crates/backend/src/companion_fwd.rs`): inherited trait members are
//! forwarded, `protected` / `private[p]` ones are not, and a name the
//! companion class already uses suppresses every alternative of that name.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-mirrorfwd-{tag}-{}-{nanos}-{n}",
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
    Command::new("java").arg("-version").output().is_ok()
}

fn javap_available() -> bool {
    Command::new("javap").arg("-version").output().is_ok()
}

fn compile(src: &Path, tag: &str, extra: &[&str]) -> PathBuf {
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {} ({tag}) failed:\n{}\n{}",
        src.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_verified(out: &Path, cp_extra: Option<&Path>, what: &str) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all failed for {what}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `java Main` on a program whose `object Main` has a companion `class Main`.
/// The fixture prints every static method `Main.class` carries, so a wrong
/// forwarder set fails as loudly as a missing one; the expected output is
/// real scalac 2.13.16's own (`tests/fixtures/expected/mirrorfwd.txt`).
#[test]
fn fixtures_mirrorfwd() {
    if !java_available() {
        eprintln!("skip mirrorfwd: no `java` on PATH");
        return;
    }
    let src = fixtures_dir().join("mirrorfwd.scala");
    let exp = fs::read_to_string(fixtures_dir().join("expected/mirrorfwd.txt")).unwrap();

    let out = compile(&src, "priv", &["--no-scala-library"]);
    assert_eq!(
        run_verified(&out, None, "private runtime"),
        exp,
        "private-runtime stdout mismatch"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mirrorfwd scala-library dual-run: jar not present");
        return;
    };
    let out = compile(&src, "lib", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_verified(&out, Some(&jar), "scala-library ABI"),
        exp,
        "scala-library stdout mismatch"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The `public static` methods `javap -p` reports for one class, sorted.
/// Overloads keep their descriptor so a dropped alternative is visible.
fn statics_of(out: &Path, class_name: &str) -> Vec<String> {
    let text = Command::new("javap")
        .args(["-p", "-classpath", out.to_str().unwrap(), class_name])
        .output()
        .expect("javap");
    assert!(
        text.status.success(),
        "javap failed for {class_name}: {}",
        String::from_utf8_lossy(&text.stderr)
    );
    let mut v: Vec<String> = String::from_utf8_lossy(&text.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("static"))
        .map(|l| {
            // `public static int twice(int);` -> `twice(int)`
            let l = l.trim_end_matches(';');
            let open = l.find('(').unwrap_or(l.len());
            let start = l[..open].rfind(' ').map(|i| i + 1).unwrap_or(0);
            l[start..].to_string()
        })
        .collect();
    v.sort();
    v
}

fn write_source(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p
}

/// Which members get forwarded, checked against real scalac 2.13.16's answer
/// for this exact source (recorded in `crates/backend/src/companion_fwd.rs`):
///
/// * `class Test` gets `own`, `abstractOne`, `fromTrait` and `traitVal` --
///   a mixed-in trait's concrete `def` and its `val` are forwarded, not just
///   what the object's own body declares;
/// * `clash` and `clashDiffSig` are not forwarded at all, because the
///   companion class uses those names -- by name, so the `clashDiffSig(Int)`
///   alternative goes too even though its signature differs;
/// * `prot` (`protected`) and `bnd` (`private[p]`) are not forwarded, though
///   both are `public` in `Test$.class`;
/// * `object Solo`, which has no companion class, still gets a mirror class
///   with the same set minus the conflicts.
#[test]
fn forwarder_selection_matches_scalac() {
    if !javap_available() {
        eprintln!("skip forwarder_selection: no `javap` on PATH");
        return;
    }
    let dir = tmp_dir("select-src");
    let src = write_source(
        &dir,
        "Sel.scala",
        r#"package p

trait T {
  def fromTrait(): Int = 1
  def abstractOne(): Int
  val traitVal: Int = 9
}

class Test {
  def clash(): Int = 100
  def onlyOnClass(): Int = 101
  def clashDiffSig(s: String): String = s
}

object Test extends T {
  def abstractOne(): Int = 3
  def clash(): Int = 200
  def clashDiffSig(i: Int): Int = i
  def own(): Int = 4
  override def toString: String = "T"
  protected def prot(): Int = 5
  private[p] def bnd(): Int = 6
}

object Solo extends T {
  def abstractOne(): Int = 3
  def own(): Int = 4
}
"#,
    );
    let out = compile(&src, "select", &["--no-scala-library"]);
    assert_eq!(
        statics_of(&out, "p.Test"),
        vec!["abstractOne()", "fromTrait()", "own()", "traitVal()"],
    );
    // No companion class, so nothing suppresses `toString` here -- scalac
    // forwards an *overridden* `toString` onto a mirror class and never onto
    // a companion class.
    assert_eq!(
        statics_of(&out, "p.Solo"),
        vec!["abstractOne()", "fromTrait()", "own()", "traitVal()"],
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&dir);
}

/// A companion `trait` takes the forwarders too: scalac writes them into the
/// interface classfile (`public static int onObj()` on `p.TC`), which needs
/// classfile major 52. And a value class's companion must not re-declare the
/// `$extension` statics the class already carries -- a duplicate method makes
/// the whole classfile unloadable, so this also checks it still parses.
#[test]
fn forwarders_on_trait_and_value_class() {
    if !javap_available() {
        eprintln!("skip forwarders_on_trait_and_value_class: no `javap` on PATH");
        return;
    }
    let dir = tmp_dir("trait-src");
    let src = write_source(
        &dir,
        "Tc.scala",
        r#"package p

trait TC {
  def onTrait(): Int = 1
}
object TC {
  def onObj(): Int = 2
  def onTrait(): Int = 3
}

class Meters(val v: Int) extends AnyVal {
  def plus(o: Int): Int = v + o
}
object Meters {
  def zero: Meters = new Meters(0)
}
"#,
    );
    let out = compile(&src, "trait", &["--no-scala-library"]);
    assert_eq!(statics_of(&out, "p.TC"), vec!["onObj()"]);
    // `zero` is forwarded; `plus$extension` is already a static of the value
    // class and must appear exactly once; `v$extension` is suppressed by the
    // class's own `v()`.
    let meters = statics_of(&out, "p.Meters");
    assert!(meters.contains(&"zero()".to_string()), "{meters:?}");
    assert_eq!(
        meters
            .iter()
            .filter(|m| m.starts_with("plus$extension"))
            .count(),
        1,
        "{meters:?}"
    );
    assert!(
        !meters.iter().any(|m| m.starts_with("v$extension")),
        "{meters:?}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&dir);
}
