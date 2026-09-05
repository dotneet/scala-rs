//! nsc 2.13's trait ABI: a trait's concrete members live on the *interface*.
//!
//! nsc compiles a concrete trait method to a `default` method holding the
//! body plus a `public static m$($this, …)` beside it (which is what every
//! mixin forwarder and every `super` call goes through, since `invokespecial`
//! on a default method would need the interface to be a direct
//! superinterface), and `$init$` to a `static` method on the interface. It
//! emits no implementation class at all.
//!
//! scala-rs used to emit the Scala 2.11 encoding instead -- one
//! `<Iface>$class` holder of statics per trait with a concrete member, 106 of
//! them for slick. Nothing a class file *we* compiled did was wrong under
//! that scheme, but a subclass compiled by real scalac could not find a
//! single one of our trait implementations, which is an ABI incompatibility
//! rather than a class file count.
//!
//! The check that actually settles it is `nsc_compiled_subclass_runs_against_
//! our_traits`: scala-rs compiles `tc_lib.scala`, real scalac 2.13.16
//! compiles `tc_app.scala` against those class files, and the pair runs and
//! prints what scalac-on-scalac prints. The shape assertions below say *why*
//! it works, so a regression names itself instead of appearing as a
//! `NoSuchMethodError` at the far end.
//!
//! Interface method bodies are also the reason this matters more than it
//! looks: a `T$class` holder is loaded only when something calls it, so a
//! body that cannot verify sits there silently (`tests/slick_subset.sh` loads
//! every class file with `Class.forName(initialize = false)`, which does not
//! link, and so never verifies a method body). An interface is verified the
//! moment any implementing class loads.

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
        "scala-rs-traitclass-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Real scalac 2.13.16, when this machine has the checkout the measurement
/// scripts install. Every test that needs it skips loudly without it.
fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn compile_fixture(name: &str, out: &Path, extra: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} extra={extra:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

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

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

// --------------------------------------------------------------------- run

/// `tc_iface` in both modes, against the stdout nsc 2.13.16 prints for it:
/// a trait `val` set from `$init$`, a `var` with a plain setter, a `private`
/// helper, a lambda in a trait body, `super` into a trait from a class, a
/// stackable `abstract override` chain, and an `object` mixing a trait in.
#[test]
fn tc_iface_matches_scalac_in_both_modes() {
    if !java_available() {
        return;
    }
    let exp = expected("tc_iface");

    let out = tmp_dir("priv");
    compile_fixture("tc_iface", &out, &["--no-scala-library"]);
    assert_eq!(
        run_main(out.to_str().unwrap()),
        exp,
        "stdout mismatch for tc_iface on the private runtime"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip tc_iface (jar): scala-library jar not present");
        return;
    };
    let out = tmp_dir("jar");
    compile_fixture(
        "tc_iface",
        &out,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_main(&format!("{}:{}", out.display(), jar.display())),
        exp,
        "stdout mismatch for tc_iface against the jar"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------- shape

/// No `<Iface>$class` holder is emitted any more, in either mode. This is the
/// one assertion that would still pass if every *call* were wrong, so it is
/// deliberately not the only one.
#[test]
fn tc_iface_emits_no_impl_class() {
    for extra in [vec!["--no-scala-library"], jar_args()] {
        if extra.is_empty() {
            continue;
        }
        let out = tmp_dir("noimpl");
        compile_fixture("tc_iface", &out, &extra);
        let leftovers: Vec<String> = fs::read_dir(&out)
            .expect("read out dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with("$class.class"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "a trait must not get an implementation class any more, got {leftovers:?}"
        );
        let _ = fs::remove_dir_all(&out);
    }
}

fn jar_args() -> Vec<&'static str> {
    match scala_library_jar() {
        // Leaked deliberately: the path is a constant, and `compile_fixture`
        // needs a `&str` that outlives the call.
        Some(_) => vec![
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        ],
        None => Vec::new(),
    }
}

/// The emitted shape, member by member, against what `javap -p` reports for
/// scalac 2.13.16's own output for the same source.
#[test]
fn tc_iface_has_nscs_trait_shape() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip tc_iface shape: scala-library jar not present");
        return;
    };
    let out = tmp_dir("shape");
    compile_fixture(
        "tc_iface",
        &out,
        &["--scala-library", jar.to_str().unwrap()],
    );
    let iface = javap(&out, "Greet");

    // A concrete member: the body in a `default` method, and the `m$` static
    // beside it whose first parameter is the receiver.
    assert!(
        iface.contains("public default java.lang.String greet();"),
        "a concrete trait method must be a default method, got:\n{iface}"
    );
    assert!(
        iface.contains("public static java.lang.String greet$(Greet);"),
        "a concrete trait method must get nsc's `m$` static, got:\n{iface}"
    );
    // `$init$` is a static on the interface, not a method of a helper class.
    assert!(
        iface.contains("public static void $init$(Greet);"),
        "`$init$` belongs on the interface, got:\n{iface}"
    );
    // A genuine `private` gets neither a declaration nor a forwarder: only
    // the `private static` body the trait's own members call. See
    // `crates/cli/tests/traitpriv.rs` for the flag-level version of this.
    assert!(
        iface.contains("private static java.lang.String punct$(Greet);"),
        "a private trait method keeps the `$this`-taking shape, got:\n{iface}"
    );
    assert!(
        !iface.contains("java.lang.String punct();"),
        "a private trait method must not be declared on the interface, got:\n{iface}"
    );
    // A lambda in a trait body hoists onto the interface, as nsc's do, and
    // its bootstrap has to name an `InterfaceMethodref`.
    assert!(
        iface.contains("$anonfun$"),
        "a lambda in a trait body belongs on the interface, got:\n{iface}"
    );

    // The implementing class forwards through the `m$` static, and runs
    // `$init$` from its constructor.
    let person = javap(&out, "Person");
    assert!(
        person.contains("InterfaceMethod Greet.greet$:(LGreet;)Ljava/lang/String;"),
        "the mixin forwarder must call the trait's `greet$`, got:\n{person}"
    );
    assert!(
        person.contains("InterfaceMethod Greet.$init$:(LGreet;)V"),
        "the constructor must call the trait's `$init$`, got:\n{person}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ----------------------------------------------------------------- interop

/// The one that settles it. scala-rs compiles the traits; **real scalac
/// 2.13.16** compiles a subclass against those class files; the two run
/// together and must print exactly what scalac-on-scalac prints.
///
/// Everything the trait ABI covers has to line up for this to pass: the
/// `default` methods and their `m$` statics (dispatch), `$init$` on the
/// interface *and* in the pickle as `nme.MIXIN_CONSTRUCTOR` (or scalac emits
/// no call and every trait `val` stays null), and a `var`'s pickled accessor
/// being non-STABLE with a `v_$eq` beside it (or scalac implements the mixin
/// `T$_setter_$v_$eq` protocol our `$init$` does not call).
#[test]
fn nsc_compiled_subclass_runs_against_our_traits() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip trait ABI interop: needs the scala-library jar and scalac 2.13.16");
        return;
    };
    if !java_available() {
        return;
    }
    let app = fixtures_dir().join("tc_app.scala");
    let lib = fixtures_dir().join("tc_lib.scala");

    // Control: scalac compiles both halves.
    let n_lib = tmp_dir("nsc-lib");
    let n_app = tmp_dir("nsc-app");
    run_scalac(
        &scalac,
        &["-d", n_lib.to_str().unwrap(), lib.to_str().unwrap()],
    );
    run_scalac(
        &scalac,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "-d",
            n_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let control = run_main(&format!(
        "{}:{}:{}",
        n_lib.display(),
        n_app.display(),
        jar.display()
    ));

    // The real thing: scala-rs compiles the traits, scalac the subclass.
    let r_lib = tmp_dir("rs-lib");
    let r_app = tmp_dir("nsc-over-rs");
    compile_fixture(
        "tc_lib",
        &r_lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    run_scalac(
        &scalac,
        &[
            "-cp",
            r_lib.to_str().unwrap(),
            "-d",
            r_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let ours = run_main(&format!(
        "{}:{}:{}",
        r_lib.display(),
        r_app.display(),
        jar.display()
    ));

    assert_eq!(
        ours, control,
        "a subclass real scalac compiled against our traits does not behave \
         like one compiled against scalac's own"
    );
    for d in [n_lib, n_app, r_lib, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

fn run_scalac(scalac: &Path, args: &[&str]) {
    let output = Command::new(scalac)
        .args(args)
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

// ------------------------------------------------- ti_: the rest of the ABI
//
// Moving trait bodies onto the interface (above) made three more pieces of
// nsc's trait ABI *reachable*. None of them is caught by the JVM verifier --
// an interface call site is not type-checked at link time -- and the third is
// not caught by anything at all: it used to leave every `val` of a trait read
// from `-cp` at its default value, with no exception and no diagnostic.
//
//   1. A trait's unqualified-`private` `val` / `var` kept its source name, so
//      the class scalac compiles implements `tilib$Counter$$n()` while ours
//      declared `n()`: `AbstractMethodError`.
//   2. A `super` accessor was not pickled (nsc's `SUPERACCESSOR`, raw
//      `1 << 28`), so scalac mixed in no layer at all for an `abstract
//      override` trait of ours and silently ran the base implementation.
//   3. `$init$` was called only for traits compiled in the same run, and the
//      `val`s of a trait read from `-cp` got no field, no accessor and no
//      initialisation.
//
// So all three are settled by *running* a mixed pair -- both ways round --
// against what scalac-on-scalac prints for the same sources.

/// scala-rs compiles `ti_lib.scala`; real scalac 2.13.16 compiles
/// `ti_app.scala` against those class files. Covers (1): the trait's
/// `private` `val`, `private[this]` `val`, `private var` and `private lazy
/// val`, plus -- as the control on how far the expansion goes -- a
/// `private[tilib]` and a `protected` one, which nsc leaves alone.
#[test]
fn ti_nsc_subclass_runs_against_our_private_trait_state() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = ti_control(&scalac, &jar, "ti_app");
    let ours = ti_forward(&scalac, &jar, "ti_app");
    assert_eq!(
        ours, control,
        "a subclass real scalac compiled against our trait's `private` state \
         does not behave like one compiled against scalac's own"
    );
}

/// Same direction, for (2): a stackable `abstract override` chain. The
/// failure this catches prints the *base* implementation and exits 0.
#[test]
fn ti_nsc_subclass_runs_against_our_stackable_trait() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = ti_control(&scalac, &jar, "ti_stack");
    let ours = ti_forward(&scalac, &jar, "ti_stack");
    assert_eq!(
        ours, control,
        "a class real scalac compiled over our `abstract override` traits does \
         not stack them the way one compiled against scalac's own does"
    );
}

/// The other direction, which is the only one (3) shows up in: **scalac**
/// compiles `ti_lib.scala`, **scala-rs** compiles `ti_app.scala` against it.
/// Every `val` and `var` of that trait needs a field, two accessors and a
/// `$init$` call on the class we emit, and `TraitImpls` -- harvested from
/// source trees -- knows nothing about a trait that arrived as a class file.
#[test]
fn ti_our_subclass_runs_against_nscs_trait() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = ti_control(&scalac, &jar, "ti_app");

    let n_lib = tmp_dir("ti-nsc-lib");
    let r_app = tmp_dir("ti-rs-over-nsc");
    run_scalac(
        &scalac,
        &[
            "-d",
            n_lib.to_str().unwrap(),
            fixtures_dir().join("ti_lib.scala").to_str().unwrap(),
        ],
    );
    compile_fixture(
        "ti_app",
        &r_app,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    let ours = run_main(&format!(
        "{}:{}:{}",
        n_lib.display(),
        r_app.display(),
        jar.display()
    ));
    assert_eq!(
        ours, control,
        "a subclass of ours over scalac's trait does not initialise the \
         trait's state the way scalac's own subclass does"
    );
    for d in [n_lib, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// The names, so a regression says which rule broke instead of surfacing as
/// an `AbstractMethodError` at the far end of the interop tests above.
#[test]
fn ti_trait_private_state_uses_nscs_expanded_names() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ti_lib shape: scala-library jar not present");
        return;
    };
    let out = tmp_dir("ti-shape");
    compile_fixture("ti_lib", &out, &["--scala-library", jar.to_str().unwrap()]);
    let iface = javap(&out, "tilib.Counter");
    for want in [
        // `private val n`
        "public abstract int tilib$Counter$$n();",
        // the mixin setter is named after the *expanded* getter
        "public abstract void tilib$Counter$_setter_$tilib$Counter$$n_$eq(int);",
        // `private[this] val seed`, expanded exactly like a plain `private`
        "public abstract java.lang.String tilib$Counter$$seed();",
        // `private var m`: a plain `v_$eq`, on the expanded name
        "public abstract int tilib$Counter$$m();",
        "public abstract void tilib$Counter$$m_$eq(int);",
        // `private lazy val doubled`. A trait `lazy val` is the one `val`
        // whose accessor is *concrete* on the interface: nsc puts the
        // initialiser in a `default` method with the usual `m$` static beside
        // it, and the implementing class's `doubled$lzycompute` calls that
        // static under its own `bitmap$0`. It stays public even though the
        // `val` is `private`, because a `private static` of one class file is
        // not callable from another.
        "public default int tilib$Counter$$doubled();",
        "public static int tilib$Counter$$doubled$(tilib.Counter);",
        // and the two nsc does *not* expand
        "public abstract int pkg();",
        "public abstract int prot();",
    ] {
        assert!(
            iface.contains(want),
            "missing `{want}` from tilib.Counter:\n{iface}"
        );
    }
    assert!(
        !iface.contains(" int n();"),
        "a trait's `private val` must not keep its source name:\n{iface}"
    );

    // The `super` accessor is declared on the interface *and* pickled; the
    // pickle half is only observable through a class scalac compiles, so it
    // is asserted in `ti_nsc_reads_our_super_accessor_and_mixin_setters…`.
    let loud = javap(&out, "tilib.Loud");
    assert!(
        loud.contains("public abstract java.lang.String tilib$Loud$$super$label();"),
        "a stackable trait must declare nsc's `super` accessor:\n{loud}"
    );
    let _ = fs::remove_dir_all(out);
}

/// scalac implements our pickled `SUPERACCESSOR` and our mixin setters -- the
/// two halves that live in the signature rather than in the class file, and so
/// are invisible to `javap` on our own output.
#[test]
fn ti_nsc_reads_our_super_accessor_and_mixin_setters_from_the_pickle() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let r_lib = tmp_dir("ti-pickle-lib");
    let r_app = tmp_dir("ti-pickle-app");
    compile_fixture(
        "ti_lib",
        &r_lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    for app in ["ti_app", "ti_stack"] {
        run_scalac(
            &scalac,
            &[
                "-cp",
                r_lib.to_str().unwrap(),
                "-d",
                r_app.to_str().unwrap(),
                fixtures_dir()
                    .join(format!("{app}.scala"))
                    .to_str()
                    .unwrap(),
            ],
        );
    }
    let sub = javap(&r_app, "Sub");
    for want in [
        "public int tilib$Counter$$n();",
        // `final` here: scalac marks the mixin setter of an immutable `val`
        // final, so the prefix is deliberately not part of the needle.
        "void tilib$Counter$_setter_$tilib$Counter$$n_$eq(",
        "public void tilib$Counter$$m_$eq(int);",
    ] {
        assert!(
            sub.contains(want),
            "scalac did not read `{want}` out of our pickle:\n{sub}"
        );
    }
    let stacked = javap(&r_app, "Stacked");
    for want in [
        "public java.lang.String tilib$Loud$$super$label();",
        "public java.lang.String tilib$Twice$$super$label();",
    ] {
        assert!(
            stacked.contains(want),
            "scalac did not implement `{want}`, so our SUPERACCESSOR never \
             reached the pickle:\n{stacked}"
        );
    }
    for d in [r_lib, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

fn interop_tools() -> Option<(PathBuf, PathBuf)> {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip ti_ interop: needs the scala-library jar and scalac 2.13.16");
        return None;
    };
    if !java_available() {
        return None;
    }
    Some((jar, scalac))
}

/// scalac compiles both halves: the stdout every `ti_` interop test compares
/// against.
fn ti_control(scalac: &Path, jar: &Path, app: &str) -> String {
    let lib = tmp_dir("ti-ctl-lib");
    let out = tmp_dir("ti-ctl-app");
    run_scalac(
        scalac,
        &[
            "-d",
            lib.to_str().unwrap(),
            fixtures_dir().join("ti_lib.scala").to_str().unwrap(),
        ],
    );
    run_scalac(
        scalac,
        &[
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            fixtures_dir()
                .join(format!("{app}.scala"))
                .to_str()
                .unwrap(),
        ],
    );
    let s = run_main(&format!(
        "{}:{}:{}",
        lib.display(),
        out.display(),
        jar.display()
    ));
    for d in [lib, out] {
        let _ = fs::remove_dir_all(d);
    }
    s
}

/// scala-rs compiles the traits, real scalac the class that mixes them in.
fn ti_forward(scalac: &Path, jar: &Path, app: &str) -> String {
    let lib = tmp_dir("ti-fwd-lib");
    let out = tmp_dir("ti-fwd-app");
    compile_fixture("ti_lib", &lib, &["--scala-library", jar.to_str().unwrap()]);
    run_scalac(
        scalac,
        &[
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            fixtures_dir()
                .join(format!("{app}.scala"))
                .to_str()
                .unwrap(),
        ],
    );
    let s = run_main(&format!(
        "{}:{}:{}",
        lib.display(),
        out.display(),
        jar.display()
    ));
    for d in [lib, out] {
        let _ = fs::remove_dir_all(d);
    }
    s
}

// -------------------------------------- bt2_: what a *class file* has to say
//
// The four holes the `ti_` slice left open all have one shape: information a
// trait carries in its signature that one side or the other was dropping.
// None is caught by the JVM verifier, and the second is not caught by
// anything -- it produced the wrong answer, exit 0, and no diagnostic.
//
//   1. A trait's `lazy val` had no interface-side initialiser. nsc compiles
//      one to a `default` method holding the right-hand side plus the usual
//      `d$` static, and the implementing class's `d$lzycompute` calls that
//      static; we declared the accessor abstract, so the first *read* of the
//      `lazy val` from a class real scalac compiled was `NoSuchMethodError:
//      'int L.d$(L)'`.
//   2. Mixing in a *binary* stackable trait emitted neither the mixin
//      forwarder nor the `T$$super$m` accessor, because both were driven by
//      `TraitImpls`, which is harvested from source trees. A class method
//      beats an interface `default`, so `new Stacked().label` silently ran
//      the base implementation.
//   3. A binary trait's `var` could not be assigned (`reassignment to val
//      count`): the reader dropped `MUTABLE`.
//   4. A binary trait's *deferred* members looked concrete (```override`
//      modifier required``): the reader dropped `DEFERRED`, which is also
//      what our own pickler was failing to write for a deferred `val` / `var`.
//
// 3 and 4 were one root -- `install_classpath` allocated every pickled member
// `Flags::EMPTY` -- and 1 and 2 are separate. All four are settled by
// *running* a mixed pair, and both ways round: 1 only shows up with scalac on
// our side of the class file, 2/3/4 only with scalac on the other.

/// (1): scala-rs compiles `bt2_lib.scala`, real scalac 2.13.16 compiles
/// `bt2_app.scala` against it. The `lazy val` reads are what fail without the
/// interface-side initialiser, and `d.dv = 7` needs our pickle to carry
/// `DEFERRED` on a trait's declared `var` (scalac otherwise asks the class
/// for an `override` modifier and the file does not compile at all).
#[test]
fn bt2_nsc_subclass_reads_our_traits_lazy_val() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = bt2_control(&scalac, &jar, "bt2_app");
    let ours = bt2_forward(&scalac, &jar, "bt2_app");
    assert_eq!(
        ours, control,
        "a subclass real scalac compiled against our trait's `lazy val` does \
         not behave like one compiled against scalac's own"
    );
}

/// (3) and (4), and the other direction: **scalac** compiles the traits,
/// **scala-rs** the subclass. Without the pickle's `MUTABLE` and `DEFERRED`
/// this does not even compile.
#[test]
fn bt2_our_subclass_assigns_and_implements_a_binary_traits_members() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = bt2_control(&scalac, &jar, "bt2_app");
    let ours = bt2_reverse(&scalac, &jar, "bt2_app");
    assert_eq!(
        ours, control,
        "our subclass of scalac's trait does not read and write its members \
         the way scalac's own subclass does"
    );
}

/// (2): the one that is silent. `Stacked` mixes in two `abstract override`
/// traits that arrived as class files; before the fix it printed `b` -- the
/// base implementation -- and exited 0.
#[test]
fn bt2_our_subclass_stacks_binary_traits() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = bt2_control(&scalac, &jar, "bt2_stack");
    assert_eq!(
        control.trim(),
        "<b><b>",
        "the control itself is wrong; the fixture no longer stacks"
    );
    let ours = bt2_reverse(&scalac, &jar, "bt2_stack");
    assert_eq!(
        ours, control,
        "our class over binary `abstract override` traits does not stack them"
    );
}

/// Forward direction of the same shape, so a regression in the trait half is
/// not mistaken for one in the class half.
#[test]
fn bt2_nsc_subclass_stacks_our_traits() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let control = bt2_control(&scalac, &jar, "bt2_stack");
    let ours = bt2_forward(&scalac, &jar, "bt2_stack");
    assert_eq!(
        ours, control,
        "a class real scalac compiled over our stackable traits does not stack them"
    );
}

/// The shapes, so a regression names the rule that broke instead of surfacing
/// as a wrong number at the far end.
#[test]
fn bt2_shapes_match_nscs() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    // Our trait: a `lazy val` is a `default` method with an `m$` static, and
    // is *not* also declared abstract (which would be a duplicate member).
    let out = tmp_dir("bt2-shape");
    compile_fixture("bt2_lib", &out, &["--scala-library", jar.to_str().unwrap()]);
    let iface = javap(&out, "bt2lib.Counter");
    for want in [
        "public default int doubled();",
        "public static int doubled$(bt2lib.Counter);",
        // a `private lazy val` publishes the same pair, under nsc's expanded
        // name: a `private static` of one class file is not callable from
        // another.
        "public default int bt2lib$Counter$$secret();",
        "public static int bt2lib$Counter$$secret$(bt2lib.Counter);",
    ] {
        assert!(
            iface.contains(want),
            "missing `{want}` from bt2lib.Counter:\n{iface}"
        );
    }
    assert!(
        !iface.contains("public abstract int doubled();"),
        "a trait `lazy val` must not also be declared abstract:\n{iface}"
    );

    // Our class over *scalac's* traits: the mixin forwarder and the `super`
    // accessor, neither of which `TraitImpls` knows anything about.
    let n_lib = tmp_dir("bt2-shape-nsc-lib");
    let r_app = tmp_dir("bt2-shape-rs-app");
    run_scalac(
        &scalac,
        &[
            "-d",
            n_lib.to_str().unwrap(),
            fixtures_dir().join("bt2_lib.scala").to_str().unwrap(),
        ],
    );
    compile_fixture(
        "bt2_stack",
        &r_app,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    let stacked = javap(&r_app, "Stacked");
    for want in [
        // the forwarder: without it `Plain.label()` wins over the `default`
        "InterfaceMethod bt2lib/Twice.label$:(Lbt2lib/Twice;)Ljava/lang/String;",
        // Twice's `super` reaches Loud, not the class
        "public java.lang.String bt2lib$Twice$$super$label();",
        "InterfaceMethod bt2lib/Loud.label$:(Lbt2lib/Loud;)Ljava/lang/String;",
        // and Loud's reaches the class
        "public java.lang.String bt2lib$Loud$$super$label();",
        "Method Plain.label:()Ljava/lang/String;",
    ] {
        assert!(
            stacked.contains(want),
            "missing `{want}` from Stacked:\n{stacked}"
        );
    }
    for d in [out, n_lib, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// scalac compiles both halves of a `bt2_` pair: the stdout to compare against.
fn bt2_control(scalac: &Path, jar: &Path, app: &str) -> String {
    bt2_run(scalac, jar, app, false, false)
}

/// scala-rs compiles the traits, real scalac the class that mixes them in.
fn bt2_forward(scalac: &Path, jar: &Path, app: &str) -> String {
    bt2_run(scalac, jar, app, true, false)
}

/// real scalac compiles the traits, scala-rs the class that mixes them in.
fn bt2_reverse(scalac: &Path, jar: &Path, app: &str) -> String {
    bt2_run(scalac, jar, app, false, true)
}

fn bt2_run(scalac: &Path, jar: &Path, app: &str, rs_lib: bool, rs_app: bool) -> String {
    let lib = tmp_dir("bt2-lib");
    let out = tmp_dir("bt2-app");
    if rs_lib {
        compile_fixture("bt2_lib", &lib, &["--scala-library", jar.to_str().unwrap()]);
    } else {
        run_scalac(
            scalac,
            &[
                "-d",
                lib.to_str().unwrap(),
                fixtures_dir().join("bt2_lib.scala").to_str().unwrap(),
            ],
        );
    }
    if rs_app {
        compile_fixture(
            app,
            &out,
            &[
                "-cp",
                lib.to_str().unwrap(),
                "--scala-library",
                jar.to_str().unwrap(),
            ],
        );
    } else {
        run_scalac(
            scalac,
            &[
                "-cp",
                lib.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
                fixtures_dir()
                    .join(format!("{app}.scala"))
                    .to_str()
                    .unwrap(),
            ],
        );
    }
    let s = run_main(&format!(
        "{}:{}:{}",
        lib.display(),
        out.display(),
        jar.display()
    ));
    for d in [lib, out] {
        let _ = fs::remove_dir_all(d);
    }
    s
}
