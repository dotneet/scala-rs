//! Syntax scalac 2.13.16 accepts that the scala-rs parser used to reject,
//! found in the scala/scala `pos` / `run` corpus, and the nearby programs
//! scalac rejects that a naive relaxation would start accepting.
//!
//! * early definitions in `new` (`new { val x = 1 } with T { .. }`), typed in
//!   the constructor context, and procedure-syntax auxiliary constructors;
//! * interpolation: `$"`, raw `\"`, escapes of triple-quoted `s` parts, `$_`,
//!   and interpolated patterns (`case s"$a-$b" =>`);
//! * SIP-27 trailing commas, `; else`, Unicode symbol operators, `@@`;
//! * implicit function literals in blocks, `*` as an infix pattern,
//!   `(xs) @ _*`, `f _ compose g`, `+` as a method name, `var x: A = _`,
//!   `@strictfp`;
//! * `-Xsource:3`: `open` / `infix`, `-_` / `+_`.
//!
//! Fixture prefix: `parse_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-parse-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `src` with scala-rs (or scalac, when given) and the options
/// `opts`, into `out`. Returns whether it compiled, and its output.
fn compile(
    src: &Path,
    out: &Path,
    jar: &Path,
    opts: &[&str],
    scalac: Option<&Path>,
) -> (bool, String) {
    let output = match scalac {
        Some(sc) => Command::new(sc)
            .args(["-nowarn", "-classpath", jar.to_str().unwrap()])
            .args(["-d", out.to_str().unwrap()])
            .args(opts)
            .arg(src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .args(opts)
            .output()
            .expect("run scala-rs compile"),
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

/// Compile fixture `name`, run `Main`, and compare with the expected output.
fn check_runs(name: &str, opts: &[&str], scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, text) = compile(&src, &out, &jar, opts, scalac_path);
    assert!(ok, "compile failed:\n{text}");
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

fn with_scalac(f: impl FnOnce(&Path)) {
    match scalac() {
        Some(sc) => f(&sc),
        None => eprintln!("skip: scalac not present"),
    }
}

#[test]
fn parse_early_definitions_run() {
    check_runs("parse_early", &[], None);
}

#[test]
fn scalac_agrees_parse_early_definitions() {
    with_scalac(|sc| check_runs("parse_early", &[], Some(sc)));
}

#[test]
fn parse_lexical_corners_run() {
    check_runs("parse_lex", &[], None);
}

#[test]
fn scalac_agrees_parse_lexical_corners() {
    with_scalac(|sc| check_runs("parse_lex", &[], Some(sc)));
}

#[test]
fn parse_expression_and_pattern_syntax_runs() {
    check_runs("parse_syntax", &[], None);
}

#[test]
fn scalac_agrees_parse_expression_and_pattern_syntax() {
    with_scalac(|sc| check_runs("parse_syntax", &[], Some(sc)));
}

#[test]
fn parse_xsource3_syntax_runs() {
    check_runs("parse_future", &["-Xsource:3"], None);
}

#[test]
fn scalac_agrees_parse_xsource3_syntax() {
    with_scalac(|sc| check_runs("parse_future", &["-Xsource:3"], Some(sc)));
}

/// Programs scalac 2.13.16 rejects next to the ones the fixtures accept:
/// `(name, options, source, a fragment of our diagnostic)`.
const REJECTED: &[(&str, &[&str], &str, &str)] = &[
    // SIP-27 needs the line break.
    (
        "call_comma",
        &[],
        "object T { def f(a: Int, b: Int) = a; f(1, 2, ) }\n",
        "expected expression",
    ),
    (
        "tuple1_comma",
        &[],
        "object T { val x = (23, ) }\n",
        "expected expression",
    ),
    (
        "param_comma",
        &[],
        "object T { def f(a: Int, ) = a }\n",
        "expected identifier",
    ),
    (
        "type_comma",
        &[],
        "object T { def f: (Int, String, ) = ??? }\n",
        "expected identifier",
    ),
    (
        "import_comma",
        &[],
        "import scala.collection.{ mutable, }\nobject T\n",
        "expected identifier",
    ),
    // A block is not a comma-separated list, line break or not.
    (
        "block_comma",
        &[],
        "object T {\n  val x = { 1,\n  }\n}\n",
        "found comma",
    ),
    // `$_` is a pattern hole only; `$<` is no hole at all.
    (
        "interp_underscore",
        &[],
        "object T { val s = s\"$_\" }\n",
        "identifier or block expected",
    ),
    (
        "interp_bad_dollar",
        &[],
        "object T { val s = s\"a$<b\" }\n",
        "invalid string interpolation $<",
    ),
    // `\"` does not end a single-quoted interpolation (2.13.6+).
    (
        "raw_escaped_quote",
        &[],
        "object T { val s = raw\"\\\" }\n",
        "no longer closes",
    ),
    // A triple-quoted `s` part is processed for escapes after the fact.
    (
        "triple_bad_escape",
        &[],
        "object T { val s = s\"\"\"\\ \"\"\" }\n",
        "invalid escape '\\ '",
    ),
    (
        "triple_terminal_escape",
        &[],
        "object T { val s = s\"\"\"\\\"\"\" }\n",
        "invalid escape at terminal index 0",
    ),
    (
        "f_pattern",
        &[],
        "object T { \"x\" match { case f\"$a\" => a } }\n",
        "extractor f",
    ),
    // `; else` needs an `if`.
    (
        "semi_else",
        &[],
        "object T { val a = 1; else 2 }\n",
        "found else",
    ),
    // A backquoted `+` is never a prefix operator, and two statements on
    // one line need a `;`.
    (
        "backquoted_unary",
        &[],
        "class C { def +(x: Int) = x; def i = `+` 42 }\n",
        "after statement",
    ),
    // Early definitions: `this` outside any class, and only fields.
    (
        "early_this_toplevel",
        &[],
        "trait T { val x: Any }\nclass C extends { val x = this } with T\n",
        "this can be used only in a class, object, or template",
    ),
    (
        "early_def",
        &[],
        "trait T\nclass C extends { def f = 1 } with T\n",
        "only concrete field definitions",
    ),
    // The reference zero is exempt; the literal `null` is not.
    (
        "null_abstract_type",
        &[],
        "abstract class L { type N <: AnyRef; val n: N = null }\n",
        "type mismatch",
    ),
    // Soft modifiers exist only under -Xsource:3, and only before a class
    // (or, for `infix`, a def / trait / type) in a template.
    ("open_without_flag", &[], "open class A\n", "open"),
    ("open_trait", &["-Xsource:3"], "open trait A\n", "open"),
    (
        "infix_val",
        &["-Xsource:3"],
        "class C { infix val a: Int = 1 }\n",
        "infix",
    ),
    (
        "open_in_block",
        &["-Xsource:3"],
        "class C { def foo: Unit = { open class E } }\n",
        "open",
    ),
    // The `private` on the line after a bodiless `class B` is `b`'s, not a
    // constructor modifier of `B`.
    (
        "next_line_modifier",
        &[],
        "object O {\n  private class B\n  private val b = 1\n}\nobject U { val x = O.b }\n",
        "cannot be accessed",
    ),
    // `@strictfp` is `scala.annotation.strictfp`, which has to be imported.
    (
        "strictfp_unimported",
        &[],
        "object T { @strictfp def f(): Int = 1 }\n",
        "not found: type strictfp",
    ),
    // `-_` is a type name only under -Xsource:3.
    (
        "variant_without_flag",
        &[],
        "object T { type `-_` = Int; val f: -_ => Int = x => x }\n",
        "",
    ),
];

fn check_rejected(scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let mut wrong = Vec::new();
    for (name, opts, src, fragment) in REJECTED {
        let dir = tmp_dir(name);
        let file = dir.join(format!("{name}.scala"));
        fs::write(&file, src).unwrap();
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let (ok, text) = compile(&file, &out, &jar, opts, scalac_path);
        if ok {
            wrong.push(format!("{name}: accepted"));
        } else if scalac_path.is_none() && !text.contains(fragment) {
            wrong.push(format!("{name}: no `{fragment}` in\n{text}"));
        }
        let _ = fs::remove_dir_all(&dir);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn parse_rejections_stay_rejected() {
    check_rejected(None);
}

#[test]
fn scalac_agrees_parse_rejections() {
    with_scalac(|sc| check_rejected(Some(sc)));
}
