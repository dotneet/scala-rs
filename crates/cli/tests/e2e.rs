//! End-to-end CLI tests against `tests/fixtures`.

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
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-e2e-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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
    assert!(
        out.join("Main$.class").is_file(),
        "Main$.class missing in {}",
        out.display()
    );
    out
}

fn compile_fixture(name: &str) -> PathBuf {
    // Private-runtime fixtures must not auto-link a discovered scala-library jar.
    compile_fixture_with(name, &["--no-scala-library"])
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn check(name: &str) {
    let out = compile_fixture(name);
    if java_available() {
        let got = run_java(&out);
        let exp = expected_stdout(name);
        assert_eq!(got, exp, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn cli_help() {
    let output = Command::new(bin()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("compile"));
    assert!(s.contains("Scala 2.13"));
    assert!(s.contains("--no-scala-library"));
}

#[test]
fn fixtures_hello() {
    check("hello");
}
#[test]
fn fixtures_arithmetic() {
    check("arithmetic");
}
#[test]
fn fixtures_class_methods() {
    check("class_methods");
}
#[test]
fn fixtures_case_match() {
    check("case_match");
}
#[test]
fn fixtures_factorial() {
    check("factorial");
}
#[test]
fn fixtures_tailrec() {
    check("tailrec");
}
#[test]
fn fixtures_deprecated() {
    check("deprecated");
}
#[test]
fn fixtures_trait_impl() {
    check("trait_impl");
}
#[test]
fn fixtures_while_loop() {
    check("while_loop");
}
#[test]
fn fixtures_do_while() {
    check("do_while");
}
#[test]
fn fixtures_eq_sync() {
    check("eq_sync");
}
#[test]
fn fixtures_string_interp() {
    check("string_interp");
}
#[test]
fn fixtures_overloading() {
    check("overloading");
}
#[test]
fn fixtures_list_for() {
    check("list_for");
}

/// `compile` with no flags should auto-link a discovered scala-library jar
/// (same as `run`), so the private runtime is not emitted.
#[test]
fn compile_auto_links_discovered_scala_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip compile autodetect: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("hello.scala");
    let out = tmp_dir("compile-autolink");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile (auto-link) failed: {status}");
    assert_no_private_stdlib(&out);
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -cp out:scala-library failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("hello")
    );
    let _ = fs::remove_dir_all(&out);
}

/// `--no-scala-library` must still emit the private runtime even when a jar
/// would otherwise be auto-found.
#[test]
fn compile_no_scala_library_emits_private_runtime() {
    let src = fixtures_dir().join("hello.scala");
    let out = tmp_dir("compile-private");
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile --no-scala-library");
    assert!(
        status.success(),
        "compile --no-scala-library failed: {status}"
    );
    assert!(
        out.join("scala/Option.class").is_file(),
        "expected private scala/Option.class under {}",
        out.display()
    );
    let _ = fs::remove_dir_all(&out);
}
#[test]
fn fixtures_option_for() {
    check("option_for");
}
#[test]
fn fixtures_lazy_val() {
    check("lazy_val");
}
#[test]
fn fixtures_implicits() {
    check("implicits");
}
#[test]
fn fixtures_generic_id() {
    check("generic_id");
}
#[test]
fn fixtures_defaults() {
    check("defaults");
}
#[test]
fn fixtures_byname() {
    check("byname");
}
#[test]
fn fixtures_trait_concrete() {
    check("trait_concrete");
}
#[test]
fn fixtures_trait_linearize() {
    check("trait_linearize");
}
#[test]
fn fixtures_try_catch() {
    check("try_catch");
}
#[test]
fn fixtures_try_finally() {
    check("try_finally");
}
#[test]
fn fixtures_type_alias() {
    check("type_alias");
}
#[test]
fn fixtures_update_assign() {
    check("update_assign");
}
#[test]
fn fixtures_nested_class() {
    check("nested_class");
}
#[test]
fn fixtures_nested_object() {
    check("nested_object");
}
#[test]
fn fixtures_anonymous() {
    check("anonymous");
}
#[test]
fn fixtures_eta() {
    check("eta");
}
#[test]
fn fixtures_existentials() {
    check("existentials");
}
#[test]
fn fixtures_existential_bounds() {
    check("existential_bounds");
}
#[test]
fn fixtures_existential_val_ok() {
    check("existential_val_ok");
}
#[test]
fn fixtures_implicit_specific() {
    check("implicit_specific");
}
#[test]
fn fixtures_lambda_lift() {
    check("lambda_lift");
}
#[test]
fn fixtures_view_bounds() {
    check("view_bounds");
}
#[test]
fn fixtures_view_bounds_class() {
    check("view_bounds_class");
}
#[test]
fn fixtures_hk_types() {
    check("hk_types");
}
#[test]
fn fixtures_app() {
    check("app");
}
#[test]
fn fixtures_delayed_init() {
    check("delayed_init");
}
#[test]
fn fixtures_implicit_inherited() {
    check("implicit_inherited");
}
#[test]
fn fixtures_implicit_nested() {
    check("implicit_nested");
}
#[test]
fn fixtures_implicit_inherit_local() {
    check("implicit_inherit_local");
}
#[test]
fn fixtures_partial_function() {
    check("partial_function");
}
#[test]
fn fixtures_private_this() {
    check("private_this");
}
#[test]
fn fixtures_protected_qual() {
    check("protected_qual");
}
#[test]
fn fixtures_defaults_still_run() {
    check("defaults");
}
#[test]
fn fixtures_super() {
    check("super");
}
#[test]
fn fixtures_sealed_match() {
    check("sealed_match");
}
#[test]
fn fixtures_unapply() {
    check("unapply");
}
#[test]
fn fixtures_value_class() {
    check("value_class");
}
#[test]
fn fixtures_predef() {
    check("predef");
}
#[test]
fn fixtures_unapply_seq() {
    check("unapply_seq");
}
#[test]
fn fixtures_trait_val() {
    check("trait_val");
}
#[test]
fn fixtures_abstract_override() {
    check("abstract_override");
}
#[test]
fn fixtures_predef_more() {
    check("predef_more");
}
#[test]
fn fixtures_sealed_non_exhaustive_is_warning() {
    check("sealed_non_exhaustive");
}

#[test]
fn fatal_warnings_makes_non_exhaustive_fail() {
    let src = fixtures_dir().join("sealed_non_exhaustive.scala");
    let out = tmp_dir("fatal-warnings");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-Xfatal-warnings",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(!status.success(), "expected -Xfatal-warnings to fail");
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

fn compile_fails_lib(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip compile_fails_lib {name}: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
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

fn compile_warns(name: &str, needle: &str) {
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
        output.status.success(),
        "expected compile of {name} to succeed with a warning, got {}",
        String::from_utf8_lossy(&output.stderr)
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

#[test]
fn fixtures_implicit_ambiguous_is_error() {
    compile_fails("implicit_ambiguous", "ambiguous implicit");
}

#[test]
fn fixtures_implicit_ambiguous_parents_is_error() {
    compile_fails("implicit_ambiguous_parents", "ambiguous implicit");
}

#[test]
fn fixtures_implicit_inherit_local_ambiguous_is_error() {
    compile_fails("implicit_inherit_local_ambiguous", "ambiguous implicit");
}

#[test]
fn fixtures_private_this_bad_is_error() {
    compile_fails("private_this_bad", "cannot be accessed");
}

#[test]
fn fixtures_protected_qual_bad_is_error() {
    compile_fails("protected_qual_bad", "cannot be accessed");
}

#[test]
fn fixtures_overload_ambiguous_is_error() {
    compile_fails("overload_ambiguous", "ambiguous overload");
}

#[test]
fn fixtures_overload_none_is_error() {
    compile_fails("overload_none", "no matching overload");
}

#[test]
fn fixtures_f_interp_bad_is_error() {
    compile_fails("f_interp_bad", "f interpolator");
}

#[test]
fn fixtures_existential_bounds_is_error() {
    compile_fails("existential_val", "unimplemented");
}

#[test]
fn fixtures_view_bounds_class_bad_is_error() {
    compile_fails("view_bounds_class_bad", "no implicit");
}

#[test]
fn fixtures_hk_bad_is_error() {
    compile_fails("hk_bad", "type parameters");
}

#[test]
fn fixtures_type_member() {
    check("type_member");
}

#[test]
fn fixtures_self_type() {
    check("self_type");
}

#[test]
fn fixtures_variance() {
    check("variance");
}

#[test]
fn fixtures_unchecked_variance() {
    check("unchecked_variance");
}

#[test]
fn fixtures_path_dependent() {
    check("path_dependent");
}

#[test]
fn fixtures_this_type() {
    check("this_type");
}

#[test]
fn fixtures_compound() {
    check("compound");
}

#[test]
fn fixtures_nlreturn() {
    check("nlreturn");
}

#[test]
fn fixtures_existential_forsome() {
    check("existential_forsome");
}

#[test]
fn fixtures_java_override() {
    check("java_override");
}

#[test]
fn fixtures_java_deprecated() {
    check("java_deprecated");
}

#[test]
fn fixtures_const_types() {
    check("const_types");
}

#[test]
fn fixtures_implicit_class() {
    check("implicit_class");
}

#[test]
fn fixtures_pkg_implicit_class() {
    check("pkg_implicit_class");
}

#[test]
fn fixtures_dynamic() {
    check("dynamic");
}

#[test]
fn fixtures_postfix_ops() {
    check("postfix_ops");
}

#[test]
fn fixtures_structural() {
    check("structural");
}

#[test]
fn fixtures_structural_update() {
    check("structural_update");
}

#[test]
fn fixtures_type_proj_bad_is_error() {
    compile_fails("type_proj_bad", "stable identifier");
}

#[test]
fn fixtures_this_type_bad_is_error() {
    compile_fails("this_type_bad", "stable identifier");
}

#[test]
fn fixtures_compound_bad_is_error() {
    // `A with B` is a legal type even for two unrelated classes; what it is
    // not is a way to reach a member neither parent declares. The template
    // rule is `mism7_mixin_bad`.
    compile_fails("compound_bad", "value c is not a member of A with B");
}

#[test]
fn fixtures_return_ctor_is_error() {
    compile_fails("return_ctor", "return outside method");
}

#[test]
fn fixtures_override_bad_is_error() {
    compile_fails("override_bad", "overrides nothing");
}

#[test]
fn fixtures_const_types_bad_is_error() {
    compile_fails("const_types_bad", "type mismatch");
}

#[test]
fn fixtures_dynamic_bad_is_error() {
    compile_fails("dynamic_bad", "language.dynamics");
}

#[test]
fn fixtures_postfix_ops_warns_without_import() {
    compile_warns("postfix_ops_bad", "postfixOps");
}

#[test]
fn fixtures_implicit_conv_warns_without_import() {
    compile_warns("implicit_conv_bad", "implicitConversions");
}

#[test]
fn fatal_warnings_makes_postfix_ops_fail() {
    let src = fixtures_dir().join("postfix_ops_bad.scala");
    let out = tmp_dir("fatal-postfix");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
            "-Xfatal-warnings",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(
        !status.success(),
        "expected -Xfatal-warnings to fail postfix without import"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_structural_bad_is_error() {
    compile_fails("structural_bad", "foo_=");
}

#[test]
fn fixtures_pkg_implicit_class_bad_is_error() {
    compile_fails("pkg_implicit_class_bad", "twice is not a member");
}

#[test]
fn fixtures_pkg_implicit_toplevel_bad_is_error() {
    compile_fails("pkg_implicit_toplevel_bad", "top-level");
}

#[test]
fn fixtures_indexedseq_queue_bad_is_error() {
    compile_fails_lib("indexedseq_queue_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops3_bad_is_error() {
    compile_fails_lib("string_ops3_bad", "noSuchStrip is not a member");
}

#[test]
fn fixtures_byte_ops_bad_is_error() {
    compile_fails_lib("byte_ops_bad", "noSuchMax is not a member");
}

#[test]
fn fixtures_arraybuffer_bad_is_error() {
    compile_fails_lib("arraybuffer_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops4_bad_is_error() {
    compile_fails_lib("string_ops4_bad", "noSuchMargin is not a member");
}

#[test]
fn fixtures_numeric_range_bad_is_error() {
    compile_fails_lib("numeric_range_bad", "noSuchMk is not a member");
}

#[test]
fn fixtures_listbuffer_bad_is_error() {
    compile_fails_lib("listbuffer_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops5_bad_is_error() {
    compile_fails_lib("string_ops5_bad", "noSuchCap is not a member");
}

#[test]
fn fixtures_short_range_bad_is_error() {
    compile_fails_lib("short_range_bad", "noSuchMk is not a member");
}

#[test]
fn fixtures_stringbuilder_bad_is_error() {
    compile_fails_lib("stringbuilder_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops6_bad_is_error() {
    compile_fails_lib("string_ops6_bad", "noSuchRight is not a member");
}

#[test]
fn fixtures_long_range_bad_is_error() {
    compile_fails_lib("long_range_bad", "noSuchMk is not a member");
}

#[test]
fn fixtures_hashmap_bad_is_error() {
    compile_fails_lib("hashmap_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops7_bad_is_error() {
    compile_fails_lib("string_ops7_bad", "noSuchStart is not a member");
}

#[test]
fn fixtures_char_range_bad_is_error() {
    compile_fails_lib("char_range_bad", "noSuchMk is not a member");
}

#[test]
fn fixtures_hashset_bad_is_error() {
    compile_fails_lib("hashset_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops8_bad_is_error() {
    compile_fails_lib("string_ops8_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_array_ops2_bad_is_error() {
    compile_fails_lib("array_ops2_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_linkedhashmap_bad_is_error() {
    compile_fails_lib("linkedhashmap_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops9_bad_is_error() {
    compile_fails_lib("string_ops9_bad", "noSuchTail is not a member");
}

#[test]
fn fixtures_array_ops3_bad_is_error() {
    compile_fails_lib("array_ops3_bad", "noSuchForeach is not a member");
}

#[test]
fn fixtures_linkedhashset_bad_is_error() {
    compile_fails_lib("linkedhashset_bad", "noSuch is not a member");
}

#[test]
fn fixtures_string_ops10_bad_is_error() {
    compile_fails_lib("string_ops10_bad", "noSuchFilter is not a member");
}

#[test]
fn fixtures_array_ops4_bad_is_error() {
    compile_fails_lib("array_ops4_bad", "noSuchMap is not a member");
}

#[test]
fn fixtures_arraydeque_bad_is_error() {
    compile_fails_lib("arraydeque_bad", "noSuch is not a member");
}

#[test]
fn fixtures_placeholder_bad_is_error() {
    compile_fails("placeholder_bad", "unbound placeholder parameter");
}

#[test]
fn fixtures_array_ops5_bad_is_error() {
    compile_fails_lib("array_ops5_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_string_ops11_bad_is_error() {
    compile_fails_lib("string_ops11_bad", "noSuchDiff is not a member");
}

#[test]
fn fixtures_placeholder2_bad_is_error() {
    compile_fails(
        "placeholder2_bad",
        "missing parameter type for expanded function",
    );
}

#[test]
fn fixtures_array_ops6_bad_is_error() {
    compile_fails_lib("array_ops6_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_string_ops12_bad_is_error() {
    compile_fails_lib("string_ops12_bad", "noSuchUpdated is not a member");
}

#[test]
fn fixtures_placeholder3_bad_is_error() {
    compile_fails("placeholder3_bad", "unbound placeholder parameter");
}

#[test]
fn fixtures_array_ops7_bad_is_error() {
    compile_fails_lib("array_ops7_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_string_ops13_bad_is_error() {
    compile_fails_lib("string_ops13_bad", "noSuchPartition is not a member");
}

#[test]
fn fixtures_array_ops8_bad_is_error() {
    compile_fails_lib("array_ops8_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_array_ops9_bad_is_error() {
    compile_fails_lib("array_ops9_bad", "noSuchHead is not a member");
}

#[test]
fn fixtures_sortedset_bad_is_error() {
    compile_fails_lib("sortedset_bad", "noSuch is not a member");
}

#[test]
fn fixtures_array_ops10_bad_is_error() {
    compile_fails_lib("array_ops10_bad", "noSuchFilter is not a member");
}

#[test]
fn fixtures_string_ops14_bad_is_error() {
    compile_fails_lib("string_ops14_bad", "noSuchSorted is not a member");
}

#[test]
fn fixtures_sortedmap_bad_is_error() {
    compile_fails_lib("sortedmap_bad", "noSuch is not a member");
}

#[test]
fn fixtures_array_ops11_bad_is_error() {
    compile_fails_lib("array_ops11_bad", "noSuchFlatMap is not a member");
}

#[test]
fn fixtures_string_ops15_bad_is_error() {
    compile_fails_lib("string_ops15_bad", "noSuchIndices is not a member");
}

#[test]
fn fixtures_bitset_bad_is_error() {
    compile_fails_lib("bitset_bad", "noSuch is not a member");
}

#[test]
fn fixtures_array_ops12_bad_is_error() {
    compile_fails_lib("array_ops12_bad", "noSuchTake is not a member");
}

#[test]
fn fixtures_string_ops16_bad_is_error() {
    compile_fails_lib("string_ops16_bad", "noSuchDropWhile is not a member");
}

#[test]
fn fixtures_breaks_bad_is_error() {
    compile_fails_lib("breaks_bad", "noSuchBreakable is not a member");
}

#[test]
fn fixtures_array_ops13_bad_is_error() {
    compile_fails_lib("array_ops13_bad", "noSuchDrop is not a member");
}

#[test]
fn fixtures_string_ops17_bad_is_error() {
    compile_fails_lib("string_ops17_bad", "noSuchFind is not a member");
}

#[test]
fn fixtures_breaks2_bad_is_error() {
    compile_fails_lib("breaks2_bad", "noSuchTryBreakable is not a member");
}

#[test]
fn fixtures_array_ops14_bad_is_error() {
    compile_fails_lib("array_ops14_bad", "noSuchFoldLeft is not a member");
}

#[test]
fn fixtures_string_ops18_bad_is_error() {
    compile_fails_lib("string_ops18_bad", "noSuchToByte is not a member");
}

#[test]
fn fixtures_bigint_bad_is_error() {
    compile_fails_lib("bigint_bad", "noSuch is not a member");
}

#[test]
fn fixtures_array_ops15_bad_is_error() {
    compile_fails_lib("array_ops15_bad", "noSuchScanLeft is not a member");
}

#[test]
fn fixtures_string_ops19_bad_is_error() {
    compile_fails_lib("string_ops19_bad", "noSuchGrouped is not a member");
}

#[test]
fn fixtures_chaining_bad_is_error() {
    compile_fails_lib("chaining_bad", "noSuchPipe is not a member");
}

#[test]
fn fixtures_array_ops16_bad_is_error() {
    compile_fails_lib("array_ops16_bad", "noSuchLast is not a member");
}

#[test]
fn fixtures_string_ops20_bad_is_error() {
    compile_fails_lib("string_ops20_bad", "noSuchAppended is not a member");
}

#[test]
fn fixtures_array_ops17_bad_is_error() {
    compile_fails_lib("array_ops17_bad", "noSuchFind is not a member");
}

#[test]
fn fixtures_string_ops21_bad_is_error() {
    compile_fails_lib("string_ops21_bad", "noSuchCompare is not a member");
}

#[test]
fn fixtures_using_bad_is_error() {
    compile_fails_lib("using_bad", "noSuchResource is not a member");
}

#[test]
fn fixtures_array_ops18_bad_is_error() {
    compile_fails_lib("array_ops18_bad", "noSuchFilterNot is not a member");
}

#[test]
fn fixtures_string_ops22_bad_is_error() {
    compile_fails_lib("string_ops22_bad", "noSuchGt is not a member");
}

#[test]
fn fixtures_using2_bad_is_error() {
    compile_fails_lib("using2_bad", "noSuchAcquire is not a member");
}

#[test]
fn fixtures_array_ops19_bad_is_error() {
    compile_fails_lib("array_ops19_bad", "noSuchZipWithIndex is not a member");
}

#[test]
fn fixtures_string_ops23_bad_is_error() {
    compile_fails_lib("string_ops23_bad", "noSuchIterator is not a member");
}

#[test]
fn fixtures_using3_bad_is_error() {
    compile_fails_lib("using3_bad", "noSuchResources is not a member");
}

#[test]
fn fixtures_array_ops20_bad_is_error() {
    compile_fails_lib("array_ops20_bad", "noSuchLengthIs is not a member");
}

#[test]
fn fixtures_string_ops24_bad_is_error() {
    compile_fails_lib("string_ops24_bad", "noSuchFlatMap is not a member");
}

#[test]
fn fixtures_view_bad_is_error() {
    compile_fails_lib("view_bad", "noSuchFill is not a member");
}

#[test]
fn fixtures_capture_var_bad_is_error() {
    compile_fails_lib("capture_var_bad", "not assignable");
}

#[test]
fn fixtures_self_type_bad_is_error() {
    compile_fails("self_type_bad", "illegal inheritance");
}

#[test]
fn fixtures_variance_bad_is_error() {
    compile_fails("variance_bad", "contravariant");
}

#[test]
fn fixtures_type_member_hk() {
    check("type_member_hk");
}

#[test]
fn fixtures_type_member_hk_bad_is_error() {
    compile_fails("type_member_hk_bad", "type parameters");
}

#[test]
fn fixtures_type_member_bounds() {
    check("type_member_bounds");
}

#[test]
fn fixtures_type_member_bounds_bad_is_error() {
    compile_fails("type_member_bounds_bad", "incompatible");
}

#[test]
fn fixtures_assign_op() {
    check("assign_op");
}

#[test]
fn fixtures_assign_op_bad_is_error() {
    compile_fails("assign_op_bad", "not a member");
}

#[test]
fn fixtures_collection_converters_bad_is_error() {
    compile_fails("collection_converters_bad", "asScala is not a member");
}

#[test]
fn fixtures_refine_hk() {
    check("refine_hk");
}

#[test]
fn fixtures_refine_hk_bad_is_error() {
    compile_fails("refine_hk_bad", "takes type parameters");
}

#[test]
fn fixtures_refine_bound() {
    check("refine_bound");
}

#[test]
fn fixtures_refine_bound_bad_is_error() {
    compile_fails("refine_bound_bad", "type mismatch");
}

#[test]
fn fixtures_hk_bounded_bad_is_error() {
    compile_fails("hk_bounded_bad", "incompatible");
}

#[test]
fn fixtures_nested_proj() {
    check("nested_proj");
}

#[test]
fn fixtures_nested_proj_bad_is_error() {
    compile_fails("nested_proj_bad", "is not a member");
}

#[test]
fn fixtures_nested_proj_abs_bad_is_error() {
    compile_fails("nested_proj_abs_bad", "is not a member");
}

#[test]
fn fixtures_type_alias_cyclic_is_error() {
    compile_fails("type_alias_bad", "illegal cyclic reference");
}

#[test]
fn fixtures_update_apply_without_apply_is_error() {
    compile_fails("update_apply_bad", "value apply is not a member");
}

#[test]
fn fixtures_tailrec_bad_is_error() {
    compile_fails("tailrec_bad", "tailrec");
}

#[test]
fn fixtures_annot_bad_is_error() {
    compile_fails("annot_bad", "annotation");
}

#[test]
fn fixtures_implicit_not_found_is_error() {
    compile_fails("implicit_not_found", "no show for Int");
}

#[test]
fn fixtures_switch() {
    check("switch");
}

#[test]
fn fixtures_switch_bad_warns() {
    compile_warns("switch_bad", "could not emit switch");
}

#[test]
fn fixtures_early_defs() {
    check("early_defs");
}

#[test]
fn fixtures_early_defs_bad_is_error() {
    compile_fails("early_defs_bad", "only concrete field definitions");
}

#[test]
fn fixtures_sam() {
    check("sam");
}

#[test]
fn fixtures_sam_bad_is_error() {
    compile_fails("sam_bad", "type mismatch");
}

#[test]
fn fixtures_volatile() {
    check("volatile");
}

#[test]
fn fixtures_inline() {
    check("inline");
}

// NOTE: `inline_bad` used to assert that `@inline val` and `@inline @noinline def`
// were compile errors ("only supported on methods" / "cannot be used together").
// Verified against real scalac 2.13.16 (see crates/cli/tests/smallgaps.rs): neither
// case is rejected -- @inline/@noinline are placement-unchecked hints for the
// bytecode optimizer scala-rs does not implement. The fixture was removed and its
// scenario now lives as a passing test, `sgap_inline`, in smallgaps.rs.

#[test]
fn fixtures_java_cp() {
    check("java_cp");
}

#[test]
fn fixtures_java_sig() {
    check("java_sig");
}

#[test]
fn fixtures_java_wild() {
    check("java_wild");
}

#[test]
fn fixtures_java_throws() {
    check("java_throws");
}

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn compile_jprot_base() -> PathBuf {
    let src = fixtures_dir().join("java/jprot/Base.java");
    let out = tmp_dir("jprot-java");
    let status = Command::new("javac")
        .args(["-d", out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("javac");
    assert!(status.success(), "javac jprot.Base failed");
    assert!(
        out.join("jprot/Base.class").is_file(),
        "jprot/Base.class missing"
    );
    out
}

#[test]
fn fixtures_java_prot() {
    if !javac_available() {
        return;
    }
    let java_cp = compile_jprot_base();
    let src = fixtures_dir().join("java_prot.scala");
    let out = tmp_dir("java_prot");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            java_cp.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("compile java_prot");
    assert!(status.success(), "compile java_prot failed");
    assert!(
        out.join("jprot/Main.class").is_file(),
        "jprot/Main.class missing"
    );
    if java_available() {
        let cp = format!("{}:{}", out.display(), java_cp.display());
        let output = Command::new("java")
            .args(["-cp", &cp, "jprot.Main"])
            .output()
            .expect("java jprot.Main");
        assert!(
            output.status.success(),
            "java jprot.Main failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let got = String::from_utf8_lossy(&output.stdout).into_owned();
        assert_eq!(got, expected_stdout("java_prot"));
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&java_cp);
}

#[test]
fn fixtures_java_prot_bad_is_error() {
    if !javac_available() {
        return;
    }
    let java_cp = compile_jprot_base();
    let src = fixtures_dir().join("java_prot_bad.scala");
    let out = tmp_dir("java_prot_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            java_cp.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("compile java_prot_bad");
    assert!(
        !output.status.success(),
        "expected compile of java_prot_bad to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains("cannot be accessed"),
        "expected cannot be accessed in diagnostics, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&java_cp);
}

#[test]
fn fixtures_native() {
    check("native");
}

#[test]
fn fixtures_native_bad_is_error() {
    compile_fails("native_bad", "cannot have a body");
}

#[test]
fn fixtures_java_enum() {
    check("java_enum");
}

#[test]
fn fixtures_java_enum_bad_is_error() {
    compile_fails("java_enum_bad", "values");
}

#[test]
fn java_enum_verifies() {
    if !java_available() {
        return;
    }
    let out = compile_fixture("java_enum");
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java -Xverify:all java_enum");
    assert!(
        output.status.success(),
        "java -Xverify:all java_enum failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("java_enum")
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_aux_ctor() {
    check("aux_ctor");
}

#[test]
fn fixtures_aux_ctor_bad_is_error() {
    compile_fails("aux_ctor_bad", "this(...)");
}

#[test]
fn fixtures_aux_ctor_stmt_bad_is_error() {
    compile_fails("aux_ctor_stmt_bad", "first statement");
}

#[test]
fn fixtures_aux_ctor_super_bad_is_error() {
    compile_fails("aux_ctor_super_bad", "super");
}

#[test]
fn aux_ctor_verifies() {
    if !java_available() {
        return;
    }
    let out = compile_fixture("aux_ctor");
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java -Xverify:all aux_ctor");
    assert!(
        output.status.success(),
        "java -Xverify:all aux_ctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("aux_ctor")
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_context_bounds_bad_is_error() {
    compile_fails("context_bounds_bad", "no implicit");
}

#[test]
fn fixtures_context_bounds_class_bad_is_error() {
    compile_fails("context_bounds_class_bad", "no implicit");
}

#[test]
fn fixtures_trait_context_bounds_is_error() {
    compile_fails(
        "trait_context_bounds",
        "traits cannot have type parameters with context bounds",
    );
}

#[test]
fn fixtures_hk_view_bounds_is_error() {
    compile_fails("hk_view_bounds", "takes type parameters");
}

#[test]
fn native_acc_flag_in_javap() {
    let out = compile_fixture("native");
    if !javap_available() {
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let output = Command::new("javap")
        .args(["-v", "-p", out.join("Main$.class").to_str().unwrap()])
        .output()
        .expect("javap");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("ACC_NATIVE") && text.contains("native"),
        "expected ACC_NATIVE on @native method, got {text}"
    );
    assert!(
        text.contains("nscNativePing"),
        "expected nscNativePing in javap, got {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn unsupported_java_classfile_on_cp_is_error() {
    let cp = tmp_dir("bad-java-cp");
    fs::write(
        cp.join("Broken.class"),
        [0xCAu8, 0xFE, 0xBA, 0xBE, 0, 0, 0, 52, 0, 2, 99],
    )
    .unwrap();
    let src_path = cp.join("use_broken.scala");
    fs::write(
        &src_path,
        "object Main {\n  def main(args: Array[String]): Unit = { new Broken() }\n}\n",
    )
    .unwrap();
    let out = tmp_dir("bad-java-cp-out");
    let output = Command::new(bin())
        .args([
            "compile",
            src_path.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            cp.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile against a broken .class to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains("unsupported classfile"),
        "expected unsupported classfile diagnostic, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&cp);
}

#[test]
fn fixtures_xml_attr_bad_is_error() {
    compile_fails("xml_attr_bad", "XML");
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

fn scala_xml_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-xml_2.13-2.3.0.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/modules/scala-xml_2.13/2.3.0/scala-xml_2.13-2.3.0.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

#[test]
fn scala_library_dual_run_hello() {
    dual_run_fixture("hello");
}

#[test]
fn scala_library_dual_run_option_for() {
    dual_run_fixture("option_for");
}

#[test]
fn scala_library_dual_run_list_for() {
    dual_run_fixture("list_for");
}

#[test]
fn scala_library_dual_run_predef() {
    dual_run_fixture("predef");
}

#[test]
fn scala_library_dual_run_unapply() {
    dual_run_fixture("unapply");
}

#[test]
fn scala_library_dual_run_unapply_seq() {
    dual_run_fixture("unapply_seq");
}

#[test]
fn scala_library_dual_run_iterator() {
    dual_run_fixture("iterator");
}

#[test]
fn scala_library_dual_run_predef_more() {
    dual_run_fixture("predef_more");
}

#[test]
fn scala_library_dual_run_map() {
    dual_run_fixture("map");
}

#[test]
fn scala_library_dual_run_vector() {
    dual_run_fixture("vector");
}

#[test]
fn scala_library_dual_run_indexedseq_queue() {
    dual_run_fixture("indexedseq_queue");
}

#[test]
fn scala_library_dual_run_int_ops() {
    dual_run_fixture("int_ops");
}

#[test]
fn scala_library_dual_run_string_ops() {
    dual_run_fixture("string_ops");
}

#[test]
fn scala_library_dual_run_list_apply() {
    dual_run_fixture("list_apply");
}

#[test]
fn scala_library_dual_run_set() {
    dual_run_fixture("set");
}

#[test]
fn scala_library_dual_run_long_ops() {
    dual_run_fixture("long_ops");
}

#[test]
fn scala_library_dual_run_seq() {
    dual_run_fixture("seq");
}

#[test]
fn scala_library_dual_run_either() {
    dual_run_fixture("either");
}

#[test]
fn scala_library_dual_run_infix_either() {
    dual_run_fixture("infix_either");
}

#[test]
fn scala_library_dual_run_float_ops() {
    dual_run_fixture("float_ops");
}

#[test]
fn scala_library_dual_run_string_ops2() {
    dual_run_fixture("string_ops2");
}

#[test]
fn scala_library_dual_run_string_ops3() {
    dual_run_fixture("string_ops3");
}

#[test]
fn scala_library_dual_run_byte_ops() {
    dual_run_fixture("byte_ops");
}

#[test]
fn scala_library_dual_run_arraybuffer() {
    dual_run_fixture("arraybuffer");
}

#[test]
fn scala_library_dual_run_string_ops4() {
    dual_run_fixture("string_ops4");
}

#[test]
fn scala_library_dual_run_numeric_range() {
    dual_run_fixture("numeric_range");
}

#[test]
fn scala_library_dual_run_listbuffer() {
    dual_run_fixture("listbuffer");
}

#[test]
fn scala_library_dual_run_string_ops5() {
    dual_run_fixture("string_ops5");
}

#[test]
fn scala_library_dual_run_short_range() {
    dual_run_fixture("short_range");
}

#[test]
fn scala_library_dual_run_stringbuilder() {
    dual_run_fixture("stringbuilder");
}

#[test]
fn scala_library_dual_run_string_ops6() {
    dual_run_fixture("string_ops6");
}

#[test]
fn scala_library_dual_run_long_range() {
    dual_run_fixture("long_range");
}

#[test]
fn scala_library_dual_run_hashmap() {
    dual_run_fixture("hashmap");
}

#[test]
fn scala_library_dual_run_string_ops7() {
    dual_run_fixture("string_ops7");
}

#[test]
fn scala_library_dual_run_char_range() {
    dual_run_fixture("char_range");
}

#[test]
fn scala_library_dual_run_hashset() {
    dual_run_fixture("hashset");
}

#[test]
fn scala_library_dual_run_string_ops8() {
    dual_run_fixture("string_ops8");
}

#[test]
fn scala_library_dual_run_array_ops2() {
    dual_run_fixture("array_ops2");
}

#[test]
fn scala_library_dual_run_linkedhashmap() {
    dual_run_fixture("linkedhashmap");
}

#[test]
fn scala_library_dual_run_string_ops9() {
    dual_run_fixture("string_ops9");
}

#[test]
fn scala_library_dual_run_array_ops3() {
    dual_run_fixture("array_ops3");
}

#[test]
fn scala_library_dual_run_linkedhashset() {
    dual_run_fixture("linkedhashset");
}

#[test]
fn scala_library_dual_run_string_ops10() {
    dual_run_fixture("string_ops10");
}

#[test]
fn scala_library_dual_run_array_ops4() {
    dual_run_fixture("array_ops4");
}

#[test]
fn scala_library_dual_run_arraydeque() {
    dual_run_fixture("arraydeque");
}

#[test]
fn scala_library_dual_run_placeholder() {
    dual_run_fixture("placeholder");
}

#[test]
fn scala_library_dual_run_array_ops5() {
    dual_run_fixture("array_ops5");
}

#[test]
fn scala_library_dual_run_string_ops11() {
    dual_run_fixture("string_ops11");
}

#[test]
fn scala_library_dual_run_placeholder2() {
    dual_run_fixture("placeholder2");
}

#[test]
fn scala_library_dual_run_array_ops6() {
    dual_run_fixture("array_ops6");
}

#[test]
fn scala_library_dual_run_string_ops12() {
    dual_run_fixture("string_ops12");
}

#[test]
fn scala_library_dual_run_placeholder3() {
    dual_run_fixture("placeholder3");
}

#[test]
fn scala_library_dual_run_array_ops7() {
    dual_run_fixture("array_ops7");
}

#[test]
fn scala_library_dual_run_string_ops13() {
    dual_run_fixture("string_ops13");
}

#[test]
fn scala_library_dual_run_array_ops8() {
    dual_run_fixture("array_ops8");
}

#[test]
fn scala_library_dual_run_array_ops9() {
    dual_run_fixture("array_ops9");
}

#[test]
fn scala_library_dual_run_sortedset() {
    dual_run_fixture("sortedset");
}

#[test]
fn scala_library_dual_run_array_ops10() {
    dual_run_fixture("array_ops10");
}

#[test]
fn scala_library_dual_run_string_ops14() {
    dual_run_fixture("string_ops14");
}

#[test]
fn scala_library_dual_run_sortedmap() {
    dual_run_fixture("sortedmap");
}

#[test]
fn scala_library_dual_run_array_ops11() {
    dual_run_fixture("array_ops11");
}

#[test]
fn scala_library_dual_run_string_ops15() {
    dual_run_fixture("string_ops15");
}

#[test]
fn scala_library_dual_run_bitset() {
    dual_run_fixture("bitset");
}

#[test]
fn scala_library_dual_run_array_ops12() {
    dual_run_fixture("array_ops12");
}

#[test]
fn scala_library_dual_run_string_ops16() {
    dual_run_fixture("string_ops16");
}

#[test]
fn scala_library_dual_run_breaks() {
    dual_run_fixture("breaks");
}

#[test]
fn scala_library_dual_run_array_ops13() {
    dual_run_fixture("array_ops13");
}

#[test]
fn scala_library_dual_run_string_ops17() {
    dual_run_fixture("string_ops17");
}

#[test]
fn scala_library_dual_run_breaks2() {
    dual_run_fixture("breaks2");
}

#[test]
fn scala_library_dual_run_array_ops14() {
    dual_run_fixture("array_ops14");
}

#[test]
fn scala_library_dual_run_string_ops18() {
    dual_run_fixture("string_ops18");
}

#[test]
fn scala_library_dual_run_bigint() {
    dual_run_fixture("bigint");
}

#[test]
fn scala_library_dual_run_array_ops15() {
    dual_run_fixture("array_ops15");
}

#[test]
fn scala_library_dual_run_string_ops19() {
    dual_run_fixture("string_ops19");
}

#[test]
fn scala_library_dual_run_chaining() {
    dual_run_fixture("chaining");
}

#[test]
fn scala_library_dual_run_capture_var() {
    dual_run_fixture("capture_var");
}

#[test]
fn scala_library_dual_run_array_ops16() {
    dual_run_fixture("array_ops16");
}

#[test]
fn scala_library_dual_run_string_ops20() {
    dual_run_fixture("string_ops20");
}

#[test]
fn scala_library_dual_run_array_ops17() {
    dual_run_fixture("array_ops17");
}

#[test]
fn scala_library_dual_run_string_ops21() {
    dual_run_fixture("string_ops21");
}

#[test]
fn scala_library_dual_run_using() {
    dual_run_fixture("using");
}

#[test]
fn scala_library_dual_run_array_ops18() {
    dual_run_fixture("array_ops18");
}

#[test]
fn scala_library_dual_run_string_ops22() {
    dual_run_fixture("string_ops22");
}

#[test]
fn scala_library_dual_run_using2() {
    dual_run_fixture("using2");
}

#[test]
fn scala_library_dual_run_array_ops19() {
    dual_run_fixture("array_ops19");
}

#[test]
fn scala_library_dual_run_string_ops23() {
    dual_run_fixture("string_ops23");
}

#[test]
fn scala_library_dual_run_using3() {
    dual_run_fixture("using3");
}

#[test]
fn scala_library_dual_run_array_ops20() {
    dual_run_fixture("array_ops20");
}

#[test]
fn scala_library_dual_run_string_ops24() {
    dual_run_fixture("string_ops24");
}

#[test]
fn scala_library_dual_run_view() {
    dual_run_fixture("view");
}

#[test]
fn scala_library_dual_run_anonymous() {
    dual_run_fixture("anonymous");
}

#[test]
fn scala_library_dual_run_eta() {
    dual_run_fixture("eta");
}

#[test]
fn scala_library_dual_run_try_util() {
    dual_run_fixture("try_util");
}

#[test]
fn scala_library_dual_run_existentials() {
    dual_run_fixture("existentials");
}

#[test]
fn scala_library_dual_run_existential_bounds() {
    dual_run_fixture("existential_bounds");
}

#[test]
fn scala_library_dual_run_existential_forsome() {
    dual_run_fixture("existential_forsome");
}

#[test]
fn scala_library_dual_run_nlreturn() {
    dual_run_fixture("nlreturn");
}

#[test]
fn scala_library_dual_run_java_override() {
    dual_run_fixture("java_override");
}

#[test]
fn scala_library_dual_run_implicit_specific() {
    dual_run_fixture("implicit_specific");
}

#[test]
fn scala_library_dual_run_lambda_lift() {
    dual_run_fixture("lambda_lift");
}

#[test]
fn scala_library_dual_run_view_bounds() {
    dual_run_fixture("view_bounds");
}

#[test]
fn scala_library_dual_run_view_bounds_class() {
    dual_run_fixture("view_bounds_class");
}

#[test]
fn scala_library_dual_run_hk_types() {
    dual_run_fixture("hk_types");
}

#[test]
fn scala_library_dual_run_app() {
    dual_run_fixture("app");
}

#[test]
fn scala_library_dual_run_delayed_init() {
    dual_run_fixture("delayed_init");
}

#[test]
fn scala_library_dual_run_implicit_inherit_local() {
    dual_run_fixture("implicit_inherit_local");
}

#[test]
fn scala_library_dual_run_partial_function() {
    dual_run_fixture("partial_function");
}

#[test]
fn scala_library_dual_run_list_collect() {
    dual_run_fixture("list_collect");
}

#[test]
fn scala_library_dual_run_string_interp() {
    dual_run_fixture("string_interp");
}

#[test]
fn scala_library_dual_run_overloading() {
    dual_run_fixture("overloading");
}

#[test]
fn scala_library_dual_run_classtag() {
    dual_run_fixture("classtag");
}

#[test]
fn scala_library_dual_run_context_bounds() {
    dual_run_fixture("context_bounds");
}

#[test]
fn scala_library_dual_run_context_bounds_class() {
    dual_run_fixture("context_bounds_class");
}

#[test]
fn scala_library_dual_run_type_member_hk() {
    dual_run_fixture("type_member_hk");
}

#[test]
fn scala_library_dual_run_refine_hk() {
    dual_run_fixture("refine_hk");
}

#[test]
fn scala_library_dual_run_refine_bound() {
    dual_run_fixture("refine_bound");
}

#[test]
fn scala_library_dual_run_nested_proj() {
    dual_run_fixture("nested_proj");
}

#[test]
fn scala_library_dual_run_type_member_bounds() {
    dual_run_fixture("type_member_bounds");
}

#[test]
fn scala_library_dual_run_assign_op() {
    dual_run_fixture("assign_op");
}

#[test]
fn scala_library_dual_run_collection_converters() {
    dual_run_fixture("collection_converters");
}

#[test]
fn scala_library_dual_run_custom_interp() {
    dual_run_fixture("custom_interp");
}

#[test]
fn scala_library_dual_run_array_ops() {
    dual_run_fixture("array_ops");
}

#[test]
fn scala_library_dual_run_const_types() {
    dual_run_fixture("const_types");
}

#[test]
fn scala_library_dual_run_implicit_class() {
    dual_run_fixture("implicit_class");
}

#[test]
fn scala_library_dual_run_pkg_implicit_class() {
    dual_run_fixture("pkg_implicit_class");
}

#[test]
fn scala_library_dual_run_structural_update() {
    dual_run_fixture("structural_update");
}

#[test]
fn scala_library_dual_run_dynamic() {
    dual_run_fixture("dynamic");
}
#[test]
fn scala_library_dual_run_postfix_ops() {
    dual_run_fixture("postfix_ops");
}
#[test]
fn scala_library_dual_run_postfix_abs() {
    dual_run_fixture("postfix_abs");
}

#[test]
fn scala_library_dual_run_enumeration() {
    dual_run_fixture("enumeration");
}

#[test]
fn scala_library_dual_run_xml_lit() {
    dual_run_xml_fixture("xml_lit");
}

#[test]
fn scala_library_dual_run_xml_attr() {
    dual_run_xml_fixture("xml_attr");
}

#[test]
fn scala_library_dual_run_xml_ns() {
    dual_run_xml_fixture("xml_ns");
}

#[test]
fn scala_library_dual_run_xml_prefix() {
    dual_run_xml_fixture("xml_prefix");
}

#[test]
fn scala_library_dual_run_xml_comment() {
    dual_run_xml_fixture("xml_comment");
}

#[test]
fn scala_library_dual_run_xml_entity() {
    dual_run_xml_fixture("xml_entity");
}

const LIBRARY_COLLIDERS: &[&str] = &[
    "scala/Option.class",
    "scala/Some.class",
    "scala/Some$.class",
    "scala/None$.class",
    "scala/Function0.class",
    "scala/Function1.class",
    "scala/PartialFunction.class",
    "scala/Tuple2.class",
    "scala/NotImplementedError.class",
    "scala/collection/immutable/List.class",
    "scala/collection/immutable/$colon$colon.class",
    "scala/collection/immutable/Nil$.class",
    "scala/collection/immutable/List$.class",
    "scala/runtime/ArrowAssoc.class",
    "scala/Predef$.class",
    "scala/collection/StringOps.class",
    "scala/collection/ArrayOps.class",
    "scala/collection/ArrayOps$.class",
    "scala/collection/WithFilter.class",
    "scala/collection/Iterator.class",
    "scala/Option$WithFilter.class",
    "scala/collection/immutable/Map.class",
    "scala/collection/immutable/Map$.class",
    "scala/collection/immutable/Vector.class",
    "scala/collection/immutable/Vector$.class",
    "scala/collection/immutable/IndexedSeq.class",
    "scala/collection/immutable/IndexedSeq$.class",
    "scala/collection/immutable/Queue.class",
    "scala/collection/immutable/Queue$.class",
    "scala/Predef$any2stringadd.class",
    "scala/Predef$ArrowAssoc.class",
    "scala/runtime/RichInt.class",
    "scala/runtime/RichLong.class",
    "scala/runtime/RichDouble.class",
    "scala/runtime/RichChar.class",
    "scala/collection/immutable/Range.class",
    "scala/collection/immutable/Set.class",
    "scala/collection/immutable/Set$.class",
    "scala/collection/immutable/SortedSet.class",
    "scala/collection/immutable/SortedSet$.class",
    "scala/collection/immutable/TreeSet.class",
    "scala/collection/immutable/TreeSet$.class",
    "scala/collection/immutable/SortedMap.class",
    "scala/collection/immutable/SortedMap$.class",
    "scala/collection/immutable/TreeMap.class",
    "scala/collection/immutable/TreeMap$.class",
    "scala/collection/immutable/BitSet.class",
    "scala/collection/immutable/BitSet$.class",
    "scala/collection/immutable/Seq.class",
    "scala/collection/immutable/Seq$.class",
    "scala/collection/immutable/LazyList.class",
    "scala/collection/immutable/LazyList$.class",
    "scala/runtime/RichFloat.class",
    "scala/runtime/RichByte.class",
    "scala/runtime/RichShort.class",
    "scala/runtime/RichBoolean.class",
    "scala/collection/mutable/ArrayBuffer.class",
    "scala/collection/mutable/ArrayBuffer$.class",
    "scala/collection/mutable/ListBuffer.class",
    "scala/collection/mutable/ListBuffer$.class",
    "scala/collection/mutable/ArrayDeque.class",
    "scala/collection/mutable/ArrayDeque$.class",
    "scala/collection/mutable/StringBuilder.class",
    "scala/collection/mutable/StringBuilder$.class",
    "scala/collection/mutable/HashMap.class",
    "scala/collection/mutable/HashMap$.class",
    "scala/collection/mutable/HashSet.class",
    "scala/collection/mutable/HashSet$.class",
    "scala/collection/mutable/LinkedHashMap.class",
    "scala/collection/mutable/LinkedHashMap$.class",
    "scala/collection/mutable/LinkedHashSet.class",
    "scala/collection/mutable/LinkedHashSet$.class",
    "scala/collection/immutable/NumericRange.class",
    "scala/collection/immutable/NumericRange$.class",
    "scala/collection/immutable/NumericRange$Inclusive.class",
    "scala/collection/immutable/NumericRange$Exclusive.class",
    "scala/util/Either.class",
    "scala/util/Left.class",
    "scala/util/Right.class",
    "scala/App.class",
    "scala/DelayedInit.class",
    "scala/util/Left$.class",
    "scala/util/Right$.class",
    "scala/util/Try.class",
    "scala/util/Try$.class",
    "scala/util/Success.class",
    "scala/util/Success$.class",
    "scala/util/Failure.class",
    "scala/util/Failure$.class",
    "scala/util/control/Breaks.class",
    "scala/util/control/Breaks$.class",
    "scala/util/control/Breaks$TryBlock.class",
    "scala/util/control/Breaks$$anon$1.class",
    "scala/math/BigInt.class",
    "scala/math/BigInt$.class",
    "scala/math/BigDecimal.class",
    "scala/math/BigDecimal$.class",
    "scala/util/ChainingOps.class",
    "scala/util/ChainingOps$.class",
    "scala/util/package$chaining$.class",
    "scala/collection/View.class",
    "scala/collection/View$.class",
    "scala/collection/SeqView.class",
    "scala/util/Using.class",
    "scala/util/Using$.class",
    "scala/util/Using$Manager.class",
    "scala/util/Using$Manager$.class",
    "scala/util/Using$Releasable.class",
    "scala/util/Using$Releasable$.class",
    "scala/util/Using$Releasable$AutoCloseableIsReleasable$.class",
    "scala/util/ChainingSyntax.class",
    "scala/runtime/IntRef.class",
    "scala/runtime/ObjectRef.class",
    "scala/runtime/LongRef.class",
    "scala/runtime/BooleanRef.class",
    "scala/util/matching/Regex.class",
    "scala/Array$.class",
    "scala/runtime/NonLocalReturnControl.class",
];

fn assert_no_private_stdlib(out: &Path) {
    for rel in LIBRARY_COLLIDERS {
        let p = out.join(rel);
        assert!(
            !p.is_file(),
            "library ABI must not emit {} (would collide with scala-library.jar)",
            p.display()
        );
    }
}

fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_no_private_stdlib(&out);
    let mut cp = format!("{}:{}", out.display(), jar.display());
    if let Some(xml) = scala_xml_jar() {
        cp.push(':');
        cp.push_str(&xml.display().to_string());
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn dual_run_xml_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let Some(xml) = scala_xml_jar() else {
        panic!(
            "scala-xml 2.13 jar not obtainable; XML literals must run against the jar (no silent skip)"
        );
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_no_private_stdlib(&out);
    let cp = format!("{}:{}:{}", out.display(), jar.display(), xml.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -cp out:scala-library:scala-xml failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library+xml dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn scala_library_flag_without_path_uses_env() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip autodetect: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("hello.scala");
    let out = tmp_dir("autodetect");
    let status = Command::new(bin())
        .env("SCALA_LIBRARY_JAR", &jar)
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
        ])
        .status()
        .expect("compile --scala-library without path");
    assert!(status.success(), "autodetect --scala-library failed");
    assert_no_private_stdlib(&out);
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn cli_run_hello() {
    if !java_available() {
        return;
    }
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["run", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("hello")
    );
}

#[test]
fn cli_run_uses_auto_found_scala_library() {
    if !java_available() {
        return;
    }
    let Some(_) = scala_library_jar() else {
        eprintln!("skip run autodetect: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("int_ops.scala");
    let output = Command::new(bin())
        .args(["run", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run without --scala-library should use auto-found jar: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("int_ops")
    );
}

#[test]
fn cli_run_no_scala_library_uses_private_runtime() {
    if !java_available() {
        return;
    }
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["run", "--no-scala-library", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run --no-scala-library failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("hello")
    );
}

#[test]
fn parse_dump_contains_module() {
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["compile", src.to_str().unwrap(), "--parse"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("Module Main"), "{s}");
}

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn scala_signature_on_compiled_object() {
    let out = compile_fixture("hello");
    if !javap_available() {
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let output = Command::new("javap")
        .args(["-v", "-p", out.join("Main$.class").to_str().unwrap()])
        .output()
        .expect("javap");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("ScalaSignature") && text.contains("bytes"),
        "expected ScalaSignature annotation in javap -v, got {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn java_deprecated_runtime_visible_on_method() {
    let out = compile_fixture("java_deprecated");
    if !javap_available() {
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let output = Command::new("javap")
        .args(["-v", "-p", out.join("Main$.class").to_str().unwrap()])
        .output()
        .expect("javap");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("Deprecated") && text.contains("Ljava/lang/Deprecated;"),
        "expected Java @Deprecated RuntimeVisibleAnnotations on method, got {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn volatile_transient_flags_in_javap() {
    let out = compile_fixture("volatile");
    if !javap_available() {
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let output = Command::new("javap")
        .args(["-v", "-p", out.join("Box.class").to_str().unwrap()])
        .output()
        .expect("javap");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("ACC_VOLATILE") && text.contains("volatile"),
        "expected volatile field in javap, got {text}"
    );
    assert!(
        text.contains("ACC_TRANSIENT") && text.contains("transient"),
        "expected transient field in javap, got {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn nlreturn_verifies() {
    if !java_available() {
        return;
    }
    let out = compile_fixture("nlreturn");
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java -Xverify:all nlreturn");
    assert!(
        output.status.success(),
        "java -Xverify:all nlreturn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("nlreturn")
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn try_finally_verifies() {
    if !java_available() {
        return;
    }
    let out = compile_fixture("try_finally");
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java -Xverify:all try_finally");
    assert!(
        output.status.success(),
        "java -Xverify:all try_finally failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("try_finally")
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn update_assign_verifies() {
    if !java_available() {
        return;
    }
    let out = compile_fixture("update_assign");
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java -Xverify:all update_assign");
    assert!(
        output.status.success(),
        "java -Xverify:all update_assign failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("update_assign")
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn separate_compilation_against_classfiles() {
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("separate_lib.scala");
    let main_src = fixtures_dir().join("separate_main.scala");
    let out_lib = tmp_dir("separate-lib");
    let out_main = tmp_dir("separate-main");
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            lib_src.to_str().unwrap(),
            "-d",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile Lib");
    assert!(status.success(), "compile separate_lib failed");
    assert!(
        out_lib.join("Lib$.class").is_file(),
        "Lib$.class missing in {}",
        out_lib.display()
    );
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            main_src.to_str().unwrap(),
            "-d",
            out_main.to_str().unwrap(),
            "-cp",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile Main against Lib classfiles");
    assert!(
        status.success(),
        "compile separate_main against Lib classfiles failed"
    );
    let cp = format!("{}:{}", out_main.display(), out_lib.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("separate")
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&out_main);
}

#[test]
fn separate_compilation_package_object_implicit_class() {
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("pkg_implicit_lib.scala");
    let main_src = fixtures_dir().join("pkg_implicit_main.scala");
    let out_lib = tmp_dir("pkg-implicit-lib");
    let out_main = tmp_dir("pkg-implicit-main");
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            lib_src.to_str().unwrap(),
            "-d",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile pkg_implicit_lib");
    assert!(status.success(), "compile pkg_implicit_lib failed");
    assert!(
        out_lib.join("enrich/package$.class").is_file(),
        "enrich/package$.class missing in {}",
        out_lib.display()
    );
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            main_src.to_str().unwrap(),
            "-d",
            out_main.to_str().unwrap(),
            "-cp",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile Main against package object classfiles");
    assert!(
        status.success(),
        "compile pkg_implicit_main against package object classfiles failed"
    );
    let cp = format!("{}:{}", out_main.display(), out_lib.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("pkg_implicit_main")
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&out_main);
}

fn classfile_major(path: &Path) -> Option<u16> {
    let b = fs::read(path).ok()?;
    if b.len() < 8 || b[0..4] != [0xca, 0xfe, 0xba, 0xbe] {
        return None;
    }
    Some(u16::from_be_bytes([b[6], b[7]]))
}

#[test]
fn classfiles_are_java8_major_52() {
    let out = compile_fixture("while_loop");
    let main = out.join("Main$.class");
    let major = classfile_major(&main).expect("read classfile major");
    assert_eq!(major, 52, "expected Java 8 classfile major 52, got {major}");
    if java_available() {
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
            .output()
            .expect("java -Xverify:all");
        assert!(
            output.status.success(),
            "java -Xverify:all failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_stdout("while_loop")
        );
    }
    if javap_available() {
        let output = Command::new("javap")
            .args(["-v", "-p", main.to_str().unwrap()])
            .output()
            .expect("javap");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            text.contains("StackMapTable") || text.contains("stack_map"),
            "expected StackMapTable in while_loop Main$, got {text}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    // Official ~20MB 2.13.16 distribution. Skip if curl/tar is unavailable.
    let tgz = PathBuf::from("/tmp/scala-2.13.16.tgz");
    let url = "https://github.com/scala/scala/releases/download/v2.13.16/scala-2.13.16.tgz";
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "30",
            "-o",
            tgz.to_str().unwrap(),
            url,
        ])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && tgz.is_file() {
        let _ = Command::new("tar")
            .args(["-xzf", tgz.to_str().unwrap(), "-C", "/tmp"])
            .status();
        if cached.is_file() {
            return Some(cached);
        }
    }
    None
}

/// scalac 2.13 against our classfiles. Tries PATH, `/tmp/scala-2.13.16`, then a
/// small official tarball download. Probes a `val`, a `def` with params,
/// `id[T]`, a `case class` via companion apply `Point(3, 4)` / term `Point`
/// (`MODULE$`) plus field accessors, extractor `unapply` so `p match { case
/// Point(a, b) => a + b }` typechecks, an `object` method taking that case
/// class, and SIP-23 literal types `val one: 1` / `def lit(x: 1)` (CONSTANTtpe).
/// Remaining pickle holes (MACRO / late-anti flags, JAVA on EXTREF -- PickleFormat
/// EXTREF is name_Ref [owner_Ref] with no flags field) are not claimed.
/// Named annot ctor-arg reorder is **not** required: scalac 2.13.16 typechecks
/// `@Ann2(b = 2, a = "ok")` pickled as positional RHS in source order. Nested `List[_ <: List[_]]` and refinement
/// `A with B { def f: Int }` are pickled so scalac 2.13.16 can typecheck
/// `Lib.nest` / `Lib.idRef`. Java `@Deprecated` on a Scala method is pickled
/// as SYMANNOT so scalac `-deprecation` sees `Lib.gone`. TREE Ident/Select/literal
/// annot args (`@Ann(foo)` / `@Ann(c.x)` / `@Ann(3)`), THIStree (`@Ann(this)`),
/// LITERALclass (`@Ann(classOf[Int])`), APPLYtree (`@Ann(ident(1))` / nested
/// `ident(ident(1))`), `this.x` / `super.foo` Select, named `@Ann(foo = 1)`
/// (nsc positional Constant), named TREE `@Ann(foo = this.x)` / `@Ann(foo = bar)`
/// (nsc positional TREE), VARARGS on `String*`,
/// and BRIDGE on an Ordered erasure bridge, plus `type T = Int` (`ALIASsym`)
/// and `Lib.usesAlias` / `Lib.T` are probed. If scalac
/// cannot read a probed shape, this test fails rather than claiming success.
#[test]
fn scalac_typechecks_against_our_classfiles_if_present() {
    let Some(scalac) = find_scalac() else {
        eprintln!(
            "scalac 2.13 not obtainable; skipping scalac-vs-our-classfiles (documented in README)"
        );
        return;
    };
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("separate_lib.scala");
    let out_lib = tmp_dir("scalac-cp-lib");
    let mut compile = Command::new(bin());
    compile.args([
        "compile",
        lib_src.to_str().unwrap(),
        "-d",
        out_lib.to_str().unwrap(),
    ]);
    if let Some(jar) = scala_library_jar() {
        compile.args(["--scala-library", jar.to_str().unwrap()]);
    } else {
        compile.arg("--no-scala-library");
    }
    let status = compile.status().expect("compile Lib for scalac");
    assert!(status.success());
    let probe = tmp_dir("scalac-probe");
    let src = probe.join("UseLib.scala");
    fs::write(
        &src,
        r#"
object UseLib {
  def main(args: Array[String]): Unit = {
    val s: String = Lib.greet("Scala", "!")
    val n: Int = Lib.magic
    val x: Int = Lib.id(42)
    val b: String = new Box("hi").get
    val p: Point = Point(3, 4)
    val q: Int = p.x + p.y
    val m: Int = p match { case Point(a, b) => a + b }
    val sum: Int = Lib.add(Point(1, 2))
    val z: Int = Lib.f(List(1, 2))
    val d: Int = Lib.g
    val hn: Int = new Holder().me.n
    val ar: Int = Lib.fAnyRef(List("a"))
    val u: Int = Lib.h(1)
    val one: 1 = Lib.one
    val lit: Int = Lib.lit(1)
    val gone: Int = Lib.gone
    val nest: Int = Lib.nest(List(List(1)))
    val y = Lib.idRef(new MixA with MixB { override def a: Int = 1; override def b: Int = 2; def f: Int = 3 })
    val mix: Int = y.a + y.b + y.f
    val mk: Int = Lib.marked
    val ms: Int = Lib.markedSel
    val ml: Int = Lib.markedLit
    val mt: Int = new Holder().markedThis
    val mc: Int = new Holder().markedClass
    val mts: Int = new Holder().markedThisSel
    val msu: Int = new Holder().markedSuper
    val ma: Int = Lib.markedApply
    val mn: Int = Lib.markedNest
    val mna: Int = Lib.markedNamed
    val mnt: Int = new Holder().markedNamedTree
    val mni: Int = Lib.markedNamedIdent
    val mro: Int = Lib.markedReorder
    val j: Int = Lib.join("a", "b")
    val cmp: Int = new OrdBox(1).compare(new OrdBox(2))
    val al: Int = Lib.usesAlias(1)
    val at: Lib.T = 1
  }
}
"#,
    )
    .unwrap();
    let output = Command::new(&scalac)
        .args([
            "-deprecation",
            "-classpath",
            out_lib.to_str().unwrap(),
            "-d",
            probe.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        output.status.success(),
        "scalac failed to typecheck against our classfiles (val / def params / id[T] / Box.get / Point(3, 4) companion apply / Lib.add / List[_] / @deprecated g / Holder.me this.type / List[_ <: AnyRef] / Int @unchecked / Lib.one : 1 / Lib.lit(1) / Java @Deprecated Lib.gone / List[_ <: List[_]] nest / MixA with MixB {{ def f: Int }} idRef / @Ann(foo) marked / @Ann(c.x) markedSel / @Ann(3) markedLit / @Ann(this) markedThis / @Ann(classOf[Int]) markedClass / @Ann(ident(1)) markedApply / @Ann(this.x) markedThisSel / @Ann(super.foo) markedSuper / @Ann(ident(ident(1))) markedNest / @Ann(foo = 1) markedNamed / @Ann(foo = this.x) markedNamedTree / @Ann(foo = bar) markedNamedIdent / @Ann2(b = 2, a = \"ok\") markedReorder (positional source-order pickle; ctor reorder not required) / Lib.join varargs / OrdBox.compare bridge / Lib.usesAlias / Lib.T ALIASsym): {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.to_lowercase().contains("gone") && err.to_lowercase().contains("deprecated"),
        "scalac -deprecation should see Java @Deprecated on Lib.gone, got {err}"
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&probe);
}
