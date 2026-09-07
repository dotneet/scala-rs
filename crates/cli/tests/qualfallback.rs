//! E2E tests for the `agent/qualfallback` slice: a parent (or any type) whose
//! *qualifier* names nothing.
//!
//! `tree_to_type`'s `TreeKind::Select` arm in `crates/typer/src/check_types.rs`
//! falls back on the **bare** simple name when `lookup_qualified_type` finds
//! nothing under the prefix. That fallback is load-bearing: a `type` alias a
//! jar class declares leaves no trace in the bytecode, so `HtmlFormat
//! .Appendable` — which Twirl writes in the parents clause of every generated
//! template, and gitbucket has 139 of them — has no symbol to find and only
//! `qualified_pickled_type_member` can answer it.
//!
//! But the fallback fired for a *typo* too, and then answered with whatever
//! happened to share the simple name. Removing `Apply.scala` from a cats build
//! made every `trait AllOps extends Ops with Apply.AllOps` resolve
//! `Apply.AllOps` to the enclosing `AllOps` — the trait being defined — so a
//! missing file became a self-inheriting trait, six of them in a 41-file
//! subset. `object D extends Gone.Real`, where an unrelated top-level `Real`
//! exists, was accepted outright.
//!
//! `Typer::qualifier_names_nothing` is the discriminator: take the fallback
//! only when the qualifier denotes *something* this pass merely cannot model.
//! A plain `Ident` qualifier that resolves to no term, type or package
//! anywhere — after `expose_unqualified` has tried every open package,
//! wildcard import and pickle — is nsc's `not found: value <q>`, and there is
//! no path there to model.
//!
//! The two directions are tested separately, because the risk runs both ways:
//! `missing_qualifier_matches_scalac` pins that the typo is now reported the
//! way scalac reports it, and `a_pickled_alias_prefix_still_resolves_and_runs`
//! pins that the shape the fallback exists for still resolves — and *runs*,
//! because a prefix rebound to the wrong alias would still have compiled.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-qualfb-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
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

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Our diagnostics for `src`, compiled with `extra`, as one string.
fn diagnostics(src: &Path, tag: &str, extra: &[&str]) -> String {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let done = cmd.output().expect("run scala-rs compile");
    let s = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stderr),
        String::from_utf8_lossy(&done.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    s
}

/// The four cases in `qualfb_missing_qualifier_bad.scala`, as real scalac
/// 2.13.16 reports them. Kept as `(line, message)` so both halves are pinned:
/// a message on the wrong line is as wrong as the wrong message.
const MISSING_QUALIFIER_ERRORS: &[(u32, &str)] = &[
    // The qualifier names nothing and the bare name resolves — to the very
    // trait being defined. This was `illegal cyclic reference involving trait
    // AllOps`.
    (31, "not found: value Missing"),
    // The qualifier names nothing and the bare name does not resolve either;
    // this one was already right, and must stay right.
    (35, "not found: value Absent"),
    // The other side of the discriminator: the qualifier *does* denote
    // something, so it keeps nsc's member message and never becomes a
    // "not found: value".
    (39, "type Nope is not a member of object Fmt"),
    // The qualifier names nothing and the bare name resolves to an unrelated
    // top-level trait. This was accepted outright — `D` silently inherited it.
    (44, "not found: value Gone"),
];

#[test]
fn missing_qualifier_matches_scalac() {
    let src = fixture("qualfb_missing_qualifier_bad");
    let diags = diagnostics(&src, "missingqual", &["--no-scala-library"]);
    for (line, msg) in MISSING_QUALIFIER_ERRORS {
        assert!(
            diags.contains(msg),
            "expected {msg:?} for qualfb_missing_qualifier_bad, got:\n{diags}"
        );
        assert!(
            diags.contains(&format!("qualfb_missing_qualifier_bad.scala:{line}:")),
            "expected an error on line {line} of qualfb_missing_qualifier_bad, got:\n{diags}"
        );
    }
    // Nothing may be silently accepted, and the wrong root must not come back:
    // resolving the qualifier by its bare name made the parent its own parent.
    assert!(
        !diags.contains("illegal cyclic reference"),
        "an unresolved qualifier was still reported as a cycle:\n{diags}"
    );
}

/// The same file through real scalac 2.13.16, so the expectations above are an
/// oracle rather than a transcription of our own output.
#[test]
fn real_scalac_reports_the_same_missing_qualifiers() {
    let Some(scalac) = find_scalac() else {
        eprintln!("skip real_scalac_reports_the_same_missing_qualifiers: no scalac");
        return;
    };
    let out = tmp_dir("scalacqual");
    let src = fixture("qualfb_missing_qualifier_bad");
    let done = Command::new(&scalac)
        .args(["-d", out.to_str().unwrap(), src.to_str().unwrap()])
        .output()
        .expect("run scalac");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stderr),
        String::from_utf8_lossy(&done.stdout)
    );
    for (line, msg) in MISSING_QUALIFIER_ERRORS {
        let want = format!("qualfb_missing_qualifier_bad.scala:{line}: error: {msg}");
        assert!(
            text.contains(&want),
            "scalac 2.13.16 did not report {want:?}; it said:\n{text}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The shape the fallback exists for: a `type` alias declared by a class in a
/// jar, named through its object in a parents clause and in the ordinary
/// signature below it. `lookup_qualified_type` cannot see it — the alias has
/// no symbol — so this only resolves through the bare-name fallback and
/// `qualified_pickled_type_member`.
///
/// Run, not merely compiled: `Predef.Map` rebound to some other `Map` would
/// have type-checked just as well.
#[test]
fn a_pickled_alias_prefix_still_resolves_and_runs() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip a_pickled_alias_prefix_still_resolves_and_runs: no scala-library jar");
        return;
    };
    let src = fixture("qualfb_pickled_alias");
    let out = tmp_dir("pickledalias");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile qualfb_pickled_alias failed");
    if !java_available() {
        eprintln!("skip running qualfb_pickled_alias: no java");
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let cp = format!("{}:{}", out.display(), jar.display());
    let got = run_main(&cp);
    assert_eq!(
        got,
        "Map(1 -> one, 2 -> two)\n\
         Map(1 -> one, 2 -> two)\n\
         plain\n\
         3,4,5\n\
         1|2\n",
        "qualfb_pickled_alias printed the wrong thing"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source compiled and run by real scalac 2.13.16, so the expected
/// output above is an oracle too.
#[test]
fn real_scalac_runs_the_pickled_alias_fixture_the_same_way() {
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real_scalac_runs_the_pickled_alias_fixture_the_same_way: no scalac or jar");
        return;
    };
    if !java_available() {
        eprintln!("skip real_scalac_runs_the_pickled_alias_fixture_the_same_way: no java");
        return;
    }
    let src = fixture("qualfb_pickled_alias");
    let out = tmp_dir("scalacalias");
    let done = Command::new(&scalac)
        .args(["-d", out.to_str().unwrap(), src.to_str().unwrap()])
        .output()
        .expect("run scalac");
    assert!(
        done.status.success(),
        "scalac 2.13.16 rejected qualfb_pickled_alias: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let got = run_main(&cp);
    assert_eq!(
        got,
        "Map(1 -> one, 2 -> two)\n\
         Map(1 -> one, 2 -> two)\n\
         plain\n\
         3,4,5\n\
         1|2\n",
        "scalac's own run of qualfb_pickled_alias printed something else"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Prefixes that denote something, in every shape, with a decoy of the same
/// simple name at top level for each. None of these reaches the fallback at
/// all — they resolve through `lookup_qualified_type` — so this is a guard on
/// the surrounding machinery rather than on the discriminator: narrowing the
/// fallback must not disturb ordinary prefix resolution, and a prefix that got
/// dropped and re-resolved by its bare name would bind a decoy and print the
/// wrong string.
#[test]
fn resolving_prefixes_still_bind_through_the_prefix() {
    let src = fixture("qualfb_prefix_shapes");
    let out = tmp_dir("prefixshapes");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile qualfb_prefix_shapes failed");
    if !java_available() {
        eprintln!("skip running qualfb_prefix_shapes: no java");
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let got = run_main(out.to_str().unwrap());
    assert_eq!(
        got,
        "Fmt.Out\n\
         deep.Nested.Inner\n\
         deep.Nested.Inner\n\
         Holder.Inner\n\
         deep.Nested.Inner\n\
         Outer.Mid.Leaf\n\
         Box#Cell\n\
         decoy Out\n\
         decoy Cell\n\
         decoy Inner\n\
         decoy Appendable\n",
        "a qualified type bound to something other than what its prefix names"
    );
    let _ = fs::remove_dir_all(&out);
}

/// And the same source through real scalac 2.13.16.
#[test]
fn real_scalac_runs_the_prefix_shapes_the_same_way() {
    let Some(scalac) = find_scalac() else {
        eprintln!("skip real_scalac_runs_the_prefix_shapes_the_same_way: no scalac");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip real_scalac_runs_the_prefix_shapes_the_same_way: no jar");
        return;
    };
    if !java_available() {
        eprintln!("skip real_scalac_runs_the_prefix_shapes_the_same_way: no java");
        return;
    }
    let src = fixture("qualfb_prefix_shapes");
    let out = tmp_dir("scalacshapes");
    let done = Command::new(&scalac)
        .args(["-d", out.to_str().unwrap(), src.to_str().unwrap()])
        .output()
        .expect("run scalac");
    assert!(
        done.status.success(),
        "scalac 2.13.16 rejected qualfb_prefix_shapes: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let got = run_main(&cp);
    assert_eq!(
        got,
        "Fmt.Out\n\
         deep.Nested.Inner\n\
         deep.Nested.Inner\n\
         Holder.Inner\n\
         deep.Nested.Inner\n\
         Outer.Mid.Leaf\n\
         Box#Cell\n\
         decoy Out\n\
         decoy Cell\n\
         decoy Inner\n\
         decoy Appendable\n",
        "scalac's own run of qualfb_prefix_shapes printed something else"
    );
    let _ = fs::remove_dir_all(&out);
}
