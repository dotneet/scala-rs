//! `trait` / `class` / `object` declared inside a method body ("local").
//!
//! A local trait's concrete members were never harvested, so no bodies
//! and no mixin forwarders were emitted at all: the code
//! type-checked, and every call went straight to `AbstractMethodError`. A
//! local declaration also got no index in its binary name, so two methods each
//! declaring a `trait Same` produced two classfiles called `Main$Same` and the
//! second silently overwrote the first.
//!
//! Every fixture is run twice: against the private runtime
//! (`--no-scala-library`) and against the real scala-library jar
//! (`--scala-library`), which must print the same thing. The `javap` tests
//! guard the *shape* of what is emitted -- a missing forwarder is invisible in
//! stdout until some other class happens to call it.

use std::collections::BTreeSet;
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
        "scala-rs-localtrait-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
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
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile and run `name` with the private runtime and with the jar; both must
/// print `tests/fixtures/expected/<name>.txt`.
fn check_both(name: &str) {
    if !java_available() {
        return;
    }
    let exp = expected_stdout(name);

    let out = compile_fixture_with(name, &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        exp,
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} library run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        exp,
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Disassembly of one emitted class.
fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// -------------------------------------------------------------------- run

#[test]
fn fixtures_lt1_local_trait_members() {
    check_both("lt1");
}

#[test]
fn fixtures_lt2_local_trait_stacking() {
    check_both("lt2");
}

#[test]
fn fixtures_lt3_local_trait_captures() {
    check_both("lt3");
}

#[test]
fn fixtures_lt4_same_name_in_two_methods() {
    check_both("lt4");
}

#[test]
fn fixtures_lt1_bad_illegal_mixin_superclass() {
    compile_fails(
        "lt1_bad",
        "illegal inheritance; superclass Other\n is not a subclass of the superclass Sup",
    );
}

// ------------------------------------------------------------------ javap

/// The reported bug: `Main$LC` had only `v()`, so `plain()` and `w()` resolved
/// to the interface's abstract declarations. The forwarder has to be there and
/// has to call the trait's implementation, and the `lazy val` has to get its
/// own field plus bitmap on the implementing class -- exactly the shape a
/// top-level trait produces.
#[test]
fn local_trait_gets_mixin_forwarders_and_default_methods() {
    let out = compile_fixture_with("lt1", &["--no-scala-library"]);

    let iface = javap(&out, "Main$L$1");
    for m in ["v()", "fixed()", "w()", "plain()"] {
        assert!(
            iface.contains(m),
            "local trait interface should declare {m}, got:\n{iface}"
        );
    }

    assert!(
        iface.contains("static java.lang.String plain$(Main$L$1)")
            && iface.contains("static void $init$(Main$L$1)"),
        "the local trait's bodies belong on the interface, got:\n{iface}"
    );

    let lc = javap(&out, "Main$LC$1");
    assert!(
        lc.contains("public java.lang.String plain();"),
        "implementing class should carry a mixin forwarder for plain(), got:\n{lc}"
    );
    assert!(
        lc.contains("InterfaceMethod Main$L$1.plain$:(LMain$L$1;)Ljava/lang/String;"),
        "the forwarder should invokestatic the trait's `plain$`, got:\n{lc}"
    );
    assert!(
        lc.contains("public java.lang.String w();") && lc.contains("bitmap$0"),
        "a trait lazy val should become a field plus bitmap on the class, got:\n{lc}"
    );
    assert!(
        lc.contains("Main$L$1$_setter_$fixed_$eq"),
        "a trait val should still get its mixin setter, got:\n{lc}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Two methods declaring the same names must not share a classfile.
#[test]
fn same_named_local_declarations_get_separate_classfiles() {
    let out = compile_fixture_with("lt4", &["--no-scala-library"]);
    for c in [
        "Main$Same$1.class",
        "Main$Same$2.class",
        "Main$SC$1.class",
        "Main$SC$2.class",
        "Main$O$1$.class",
        "Main$O$2$.class",
        "Main$P$1.class",
        "Main$P$2.class",
    ] {
        assert!(
            out.join(c).is_file(),
            "{c} missing; local declarations must be indexed. Emitted: {:?}",
            emitted_classes(&out)
        );
    }
    assert!(
        !out.join("Main$Same.class").is_file(),
        "un-indexed Main$Same.class means two local traits shared one classfile"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A local trait reading an enclosing-method local: the trait declares an
/// accessor, and every class mixing it in implements it from its own capture
/// field. Without the accessor the trait body read a field of a class named
/// after the enclosing *method* (`getfield capturesVal.n`), which does not
/// exist.
#[test]
fn local_trait_captures_go_through_an_accessor() {
    let out = compile_fixture_with("lt3", &["--no-scala-library"]);
    let iface = javap(&out, "Main$Cap$1");
    let acc = capture_accessors(&iface);
    assert!(
        !acc.is_empty(),
        "a capturing local trait should declare capture accessors, got:\n{iface}"
    );
    let cls = javap(&out, "Main$CapC$1");
    for a in &acc {
        assert!(
            cls.contains(&format!("public int {a}();"))
                || cls.contains(&format!("public java.lang.String {a}();")),
            "implementing class should define capture accessor {a}, got:\n{cls}"
        );
    }
    for a in &acc {
        assert!(
            iface.contains(&format!("InterfaceMethod {a}:")),
            "the trait body should read {a} back through the interface \
             accessor (javap omits the owner when it is this very class), \
             got:\n{iface}"
        );
        let body = cls
            .split(&format!("{a}();"))
            .nth(1)
            .unwrap_or_else(|| panic!("no accessor body for {a}:\n{cls}"));
        assert!(
            body.split("\n\n").next().unwrap_or("").contains("getfield"),
            "capture accessor {a} should read this class's own field, got:\n{cls}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Names like `public abstract int n$37();` on a trait interface.
fn capture_accessors(iface: &str) -> Vec<String> {
    iface
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("public abstract ")?;
            let (_, rest) = l.split_once(' ')?;
            let name = rest.strip_suffix("();")?;
            // `n$37` -- a captured local's accessor, not `v()` or `plain()`.
            let (base, idx) = name.rsplit_once('$')?;
            if base.is_empty() || idx.is_empty() || !idx.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

fn emitted_classes(out: &Path) -> BTreeSet<String> {
    fs::read_dir(out)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".class"))
                .collect()
        })
        .unwrap_or_default()
}

// ------------------------------------------------- comparison against scalac

fn real_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Every *public* member of `class` in `out`, with the local-declaration index
/// stripped (`Main$L$1$_setter_$x_$eq` and nsc's `Main$L$_setter_$x_$eq` are
/// the same member; nsc drops the index when it encodes the owner). Private
/// helpers are left out: nsc splits a `lazy val` into `w()` plus a private
/// `w$lzycompute()`, we inline it, and neither is observable.
fn public_members(out: &Path, class: &str) -> BTreeSet<String> {
    let text = javap(out, class);
    text.lines()
        .map(str::trim)
        // Methods only: nsc makes a trait `val`'s field private, we make it
        // public, and a field is not a member anyone can call.
        .filter(|l| l.starts_with("public ") && l.ends_with(';') && l.contains('('))
        // `static X$(iface)` bridges are one compiler-internal entry point
        // per concrete member, not part of the member set being compared.
        .filter(|l| !l.starts_with("public static"))
        .map(|l| strip_super_owner(strip_local_index(l)))
        .collect()
}

/// `super` accessors: both compilers encode the whole binary name of the
/// owning trait (`Main$B$$super$name`), but a *local* trait carries an index
/// the two number differently, so compare only the suffix.
fn strip_super_owner(line: String) -> String {
    match line.find("$$super$") {
        None => line,
        Some(i) => {
            let start = line[..i].rfind(' ').map(|p| p + 1).unwrap_or(0);
            format!("{}{}", &line[..start], &line[i..])
        }
    }
}

/// `Main$L$1$_setter_$fixed_$eq` and nsc's `Main$L$_setter_$fixed_$eq` name the
/// same member: nsc drops a local declaration's index when it encodes the
/// owner into a member name, we keep it. Remove every `$<digits>` run that is
/// followed by `$`, so both sides read `Main$L$_setter_$fixed_$eq`.
fn strip_local_index(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'$' {
                i = j; // drop `$<digits>`, keep the `$` that follows
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

/// nsc is the reference for *which members exist*: run output alone cannot see
/// a forwarder nobody in the fixture happens to call. Skipped when the real
/// compiler is not unpacked in /tmp.
#[test]
fn implementing_class_members_match_scalac() {
    let Some(scalac) = real_scalac() else {
        eprintln!("skip javap comparison: /tmp/scala-2.13.16/bin/scalac not present");
        return;
    };
    for (fixture, classes) in [
        ("lt1", &["Main$LC$1"][..]),
        (
            "lt2",
            &["Main$ABC$1", "Main$ACB$1", "Main$K$1", "Main$Both$1"][..],
        ),
        ("lt4", &["Main$SC$1", "Main$SC$2"][..]),
    ] {
        let src = fixtures_dir().join(format!("{fixture}.scala"));
        let sc_out = tmp_dir(&format!("{fixture}-scalac"));
        let status = Command::new(&scalac)
            .args(["-d", sc_out.to_str().unwrap(), src.to_str().unwrap()])
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac failed on {fixture}");
        let rs_out = compile_fixture_with(fixture, &["--no-scala-library"]);
        for class in classes {
            let want = public_members(&sc_out, class);
            let got = public_members(&rs_out, class);
            let missing: Vec<&String> = want.difference(&got).collect();
            assert!(
                missing.is_empty(),
                "{fixture}: {class} is missing members nsc emits: {missing:?}\n\
                 nsc: {want:?}\nscala-rs: {got:?}"
            );
        }
        let _ = fs::remove_dir_all(&sc_out);
        let _ = fs::remove_dir_all(&rs_out);
    }
}
