//! `InnerClasses` (JVMS §4.7.6) and `EnclosingMethod` (JVMS §4.7.7).
//!
//! Without these, every classfile scala-rs emits for a nested class, trait,
//! object, case-class companion, value class, local class or anonymous class
//! is invisible to `java.lang.Class` reflection: `getSimpleName` returns the
//! mangled binary name (`Main$Circle` instead of `Circle`), `isMemberClass`
//! is always `false`, and `getEnclosingClass`/`getDeclaringClass` are always
//! `null`. Real scalac 2.13.16 emits both attributes; this file checks that
//! scala-rs now does too, in two independent ways:
//!
//! - **runtime dual-run**: each fixture's `main` calls the affected
//!   `java.lang.Class` methods itself and prints the results, compiled in
//!   both ABI modes and checked against real scalac's own recorded output
//!   (`tests/fixtures/expected/<name>.txt`) — see `../../tests/slick_measure.sh`
//!   and `outer.rs` for the same pattern.
//! - **`javap -v` structural comparison**: `javap_inner_classes` parses the
//!   `InnerClasses:` section scala-rs's own classfiles print and checks it
//!   against entries hand-verified against real scalac 2.13.16's `javap -v`
//!   output for the identical source (see the table in this file's PR
//!   description / commit message). Runtime behavior alone would not catch
//!   a *missing* entry for a class nobody happens to reflect on, or a wrong
//!   `access_flags` bit that no `isMemberClass`-style query surfaces.
//!
//! One naming difference from real scalac is normalized away rather than
//! matched: scalac splits `object Main` into an implementation class `Main$`
//! and a static-forwarder "mirror" class `Main`, and a member's
//! `InnerClasses` entry names the *mirror* (`... of class Main`) as its
//! owner. scala-rs has the same two classfiles, but a member's owner is
//! `Main$`, its actual runtime container (`... of class Main$`) — both are
//! internally consistent, just naming the pair's two halves differently, so
//! the comparison below checks *that* an owner is named, not its exact
//! spelling.

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
        "scala-rs-inner-{tag}-{}-{nanos}-{n}",
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile(name: &str, tag: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
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
        "compile {name} ({tag}) failed: {}",
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
        "java -Xverify:all failed for {what}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Run the fixture in both ABIs and compare against scalac's own output.
fn check_both_abis(name: &str) {
    if !java_available() {
        eprintln!("skip {name}: no `java` on PATH");
        return;
    }
    let exp = expected_stdout(name);

    let out = compile(name, &format!("{name}-priv"), &["--no-scala-library"]);
    assert_eq!(
        run_verified(&out, None, "private runtime"),
        exp,
        "private-runtime stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run for {name}: jar not present");
        return;
    };
    let out = compile(
        name,
        &format!("{name}-lib"),
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_verified(&out, Some(&jar), "scala-library ABI"),
        exp,
        "scala-library stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The reported bug, reproduced exactly: `getClass.getSimpleName` on a
/// `Circle` returned `Main$Circle`, not `Circle`, because scala-rs emitted no
/// `InnerClasses` attribute at all.
#[test]
fn fixtures_inner() {
    check_both_abis("inner");
}

/// Anonymous and local classes: `isMemberClass` must be `false` for both
/// (JVMS: `outer_class_info_index` zero), and `getSimpleName` empty only for
/// the anonymous one.
#[test]
fn fixtures_inner_local() {
    check_both_abis("inner_local");
}

/// A class/object nested in a plain (non-module) class: non-static
/// (`$outer`-carrying) nesting, and a `private` nested class's entry must
/// say `private`, not `public`.
#[test]
fn fixtures_inner_nested() {
    check_both_abis("inner_nested");
}

/// A case class's companion module and a value class.
#[test]
fn fixtures_inner_case() {
    check_both_abis("inner_case");
}

// ---------------------------------------------------------------------------
// `javap -v` structural comparison
// ---------------------------------------------------------------------------

/// One `InnerClasses` entry, with pool indices and (see the module doc
/// comment) the specific owner name stripped out — only whether an owner is
/// named matters here, not its exact spelling.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InnerEntry {
    /// Empty for an anonymous class (JVMS: `inner_name_index` zero).
    simple_name: String,
    /// Source-level modifier keywords, e.g. `"public static"`.
    flags: String,
    /// Whether `outer_class_info_index` is nonzero.
    has_outer: bool,
}

impl InnerEntry {
    fn member(name: &str, flags: &str) -> Self {
        InnerEntry {
            simple_name: name.to_string(),
            flags: flags.to_string(),
            has_outer: true,
        }
    }

    fn local(name: &str, flags: &str) -> Self {
        InnerEntry {
            simple_name: name.to_string(),
            flags: flags.to_string(),
            has_outer: false,
        }
    }

    fn anon(flags: &str) -> Self {
        InnerEntry {
            simple_name: String::new(),
            flags: flags.to_string(),
            has_outer: false,
        }
    }
}

/// Run `javap -v -p` on exactly one classfile and return its stdout.
///
/// `javap`, given a path like `Main$Shape.class`, does not always open that
/// literal file: if the same directory also has `Main$Shape$class.class`
/// (nsc-style mixin helpers make this common), it silently resolves to that
/// *other* file instead — reproducible even with an absolute, correctly
/// quoted path, seemingly because it treats the basename as a binary class
/// name to search for rather than as a literal file. Isolating the target
/// file in its own empty directory first sidesteps the ambiguity entirely.
fn run_javap(class_file: &Path) -> String {
    let iso = tmp_dir("javap-iso");
    let target = iso.join(class_file.file_name().unwrap());
    fs::copy(class_file, &target).expect("copy classfile for javap isolation");
    let output = Command::new("javap")
        .args(["-v", "-p", target.to_str().unwrap()])
        .output()
        .expect("run javap");
    assert!(
        output.status.success(),
        "javap failed for {}: {}",
        class_file.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&iso);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Parse the `InnerClasses:` section of `javap -v -p <class_file>`.
///
/// Every entry line has the shape
/// `  <flags> #N[= #M][ of #K];           // [Name=]class Bin$Name[ of class Bin]`
/// (`javap`'s own rendering); this keeps the flag keywords and the `//`
/// comment, and drops the `#N`-style pool indices, which are meaningless
/// across two different compilers' constant pools.
fn javap_inner_classes(class_file: &Path) -> Vec<InnerEntry> {
    let text = run_javap(class_file);
    let mut entries = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        if line.trim() == "InnerClasses:" {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        let t = line.trim();
        // Every entry line has both a pool reference and a terminating `;`;
        // the first line lacking either ends the attribute (the next
        // attribute's own header, e.g. `EnclosingMethod: #7.#9 // ...` has a
        // `#` but no `;`, and `SourceFile: "..."` has neither).
        if !t.contains('#') || !t.contains(';') {
            break;
        }
        let Some(comment_at) = line.find("// ") else {
            break;
        };
        let flags = line[..line.find('#').unwrap_or(0)].trim().to_string();
        let comment = line[comment_at + 3..].trim();
        let has_outer = comment.contains(" of ");
        let simple_name = match comment.find('=') {
            Some(eq) => comment[..eq].trim().to_string(),
            None => String::new(),
        };
        entries.push(InnerEntry {
            simple_name,
            flags,
            has_outer,
        });
    }
    entries.sort();
    entries
}

fn assert_inner_classes(class_file: &Path, mut expected: Vec<InnerEntry>) {
    expected.sort();
    let actual = javap_inner_classes(class_file);
    assert_eq!(
        actual,
        expected,
        "InnerClasses shape mismatch for {}",
        class_file.display()
    );
}

/// `Main$Circle` (`class Circle extends Shape`, both nested in `object
/// Main`): lists itself *and* `Shape`, since `Shape`'s `CONSTANT_Class`
/// appears in `Circle`'s own pool (`implements Main$Shape`) — real scalac's
/// `javap -v` output for the identical source:
/// ```text
/// InnerClasses:
///   public static #10= #2 of #9;   // Circle=class Main$Circle of class Main
///   public static #11= #6 of #9;   // Shape=class Main$Shape of class Main
/// ```
#[test]
fn inner_circle_lists_self_and_shape() {
    if !javap_available() {
        eprintln!("skip: no `javap` on PATH");
        return;
    }
    let out = compile("inner", "circle-javap", &["--no-scala-library"]);
    assert_inner_classes(
        &out.join("Main$Circle.class"),
        vec![
            InnerEntry::member("Circle", "public static"),
            InnerEntry::member("Shape", "public static"),
        ],
    );
    let _ = fs::remove_dir_all(&out);
}

/// `Main$Shape` only lists itself: nothing else nested is referenced in its
/// own constant pool. Real scalac agrees (`javap -v Main$Shape.class` prints
/// a single self-entry).
#[test]
fn inner_shape_lists_only_self() {
    if !javap_available() {
        eprintln!("skip: no `javap` on PATH");
        return;
    }
    let out = compile("inner", "shape-javap", &["--no-scala-library"]);
    assert_inner_classes(
        &out.join("Main$Shape.class"),
        vec![InnerEntry::member("Shape", "public static")],
    );
    let _ = fs::remove_dir_all(&out);
}

/// The anonymous class (`new Shape { ... }` inside `def make()`): a
/// self-entry with no name and `final`, no outer, plus `EnclosingMethod` —
/// *and* a `Shape` entry too, since `Shape`'s `CONSTANT_Class` appears in the
/// anonymous class's own pool (`implements Main$Shape`), exactly like
/// `Main$Circle` in `inner_circle_lists_self_and_shape`. Real scalac:
/// `public final #2;  // class Main$$anon$1`,
/// `public static #15= #6 of #14;  // Shape=class Main$Shape of class Main`,
/// `EnclosingMethod: #9.#12  // Main$.make`.
#[test]
fn inner_local_anon_has_no_outer_and_is_final() {
    if !javap_available() {
        eprintln!("skip: no `javap` on PATH");
        return;
    }
    let out = compile("inner_local", "anon-javap", &["--no-scala-library"]);
    let anon = out.join("Main$$anon$1.class");
    assert_inner_classes(
        &anon,
        vec![
            InnerEntry::anon("public final"),
            InnerEntry::member("Shape", "public static"),
        ],
    );
    assert_has_enclosing_method(&anon);
    let _ = fs::remove_dir_all(&out);
}

/// The local class (`class LocalC(...)` inside `def main`): a self-entry
/// with its real name and no outer, plus `EnclosingMethod`. Real scalac:
/// `public #11= #2;  // LocalC$1=class Main$LocalC$1` (scala-rs does not yet
/// append nsc's disambiguating `$1` suffix to a local class's own name — a
/// pre-existing, unrelated naming gap — so this checks the un-suffixed name
/// scala-rs actually emits).
#[test]
fn inner_local_class_has_no_outer() {
    if !javap_available() {
        eprintln!("skip: no `javap` on PATH");
        return;
    }
    let out = compile("inner_local", "localc-javap", &["--no-scala-library"]);
    let local = out.join("Main$LocalC.class");
    assert_inner_classes(&local, vec![InnerEntry::local("LocalC", "public")]);
    assert_has_enclosing_method(&local);
    let _ = fs::remove_dir_all(&out);
}

/// A class nested in a plain (non-module) class carries an `$outer` field,
/// so nsc leaves `ACC_STATIC` off — unlike the same shape nested in an
/// `object` (see `inner_circle_lists_self_and_shape`, `"public static"`).
/// A `private` nested class must say `private`, not `public`.
#[test]
fn inner_nested_in_class_is_not_static() {
    if !javap_available() {
        eprintln!("skip: no `javap` on PATH");
        return;
    }
    let out = compile("inner_nested", "outer-javap", &["--no-scala-library"]);
    assert_inner_classes(
        &out.join("Outer$Inner.class"),
        vec![InnerEntry::member("Inner", "public")],
    );
    assert_inner_classes(
        &out.join("Outer$PrivC.class"),
        vec![InnerEntry::member("PrivC", "private")],
    );
    let _ = fs::remove_dir_all(&out);
}

/// A case class's companion module class lists itself, `static` (nested in
/// `object Main`) and *not* `final` — nsc never sets `ACC_FINAL` for a
/// module class's own `InnerClasses` entry, even though the classfile's real
/// `access_flags` has it (an object's `final` is implicit, not written).
/// It also lists `Point` itself: the synthetic `apply(Int,Int)Point` this
/// companion carries returns `Point`, so `Point`'s `CONSTANT_Class` is in
/// this classfile's own pool too — matching real scalac's `javap -v`:
/// `public static #13= #10 of #12;  // Point=class Main$Point of class Main`
/// `public static #14= #2 of #12;   // Point$=class Main$Point$ of class Main`.
#[test]
fn inner_case_companion_is_static_not_final() {
    if !javap_available() {
        eprintln!("skip: no `javap` on PATH");
        return;
    }
    let out = compile("inner_case", "point-javap", &["--no-scala-library"]);
    assert_inner_classes(
        &out.join("Main$Point$.class"),
        vec![
            InnerEntry::member("Point", "public static"),
            InnerEntry::member("Point$", "public static"),
        ],
    );
    let _ = fs::remove_dir_all(&out);
}

/// `EnclosingMethod` (JVMS §4.7.7) must be present for a local/anonymous
/// class — without it, `getEnclosingClass` on one has nothing to fall back
/// to and incorrectly returns `null`, even though `InnerClasses` correctly
/// says it is not a member class.
fn assert_has_enclosing_method(class_file: &Path) {
    let text = run_javap(class_file);
    assert!(
        text.contains("EnclosingMethod:"),
        "{} is missing an EnclosingMethod attribute:\n{text}",
        class_file.display()
    );
}
