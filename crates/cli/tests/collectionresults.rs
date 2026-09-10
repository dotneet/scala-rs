//! Collection loading order, binary value classes, and real JDK String members.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("collresults_{name}.scala"))
}
fn matrix(names: &[(&str, bool)], warm: bool) {
    matrix_cp(names, warm, JAR);
}
fn matrix_cp(names: &[(&str, bool)], warm: bool, cp: &str) {
    matrix_ordered(names, warm.then_some("warm"), cp);
}
fn matrix_ordered(names: &[(&str, bool)], warm: Option<&str>, cp: &str) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "collection-results-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    for &(name, accepted) in names {
        let mut oracle = None;
        for nsc in [true, false] {
            for order in 0..if warm.is_some() { 3 } else { 1 } {
                let out = root.join(format!("{name}-{nsc}-{order}"));
                fs::create_dir(&out).unwrap();
                let mut c = Command::new(if nsc {
                    "/tmp/scala-2.13.16/bin/scalac"
                } else {
                    env!("CARGO_BIN_EXE_scala-rs")
                });
                if !nsc {
                    c.args(["compile", "--scala-library", JAR]);
                }
                c.args(["-cp", cp, "-d"]).arg(&out);
                if order == 1 {
                    c.arg(fixture(warm.unwrap()));
                }
                c.arg(fixture(name));
                if order == 2 {
                    c.arg(fixture(warm.unwrap()));
                }
                let p = c.output().unwrap();
                assert_eq!(
                    p.status.success(),
                    accepted,
                    "{name} nsc={nsc} order={order}: {}",
                    String::from_utf8_lossy(&p.stderr)
                );
                if accepted {
                    let p = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{cp}", out.display()),
                            "Main",
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        p.status.success(),
                        "{name} nsc={nsc} order={order}: {}",
                        String::from_utf8_lossy(&p.stderr)
                    );
                    let expected = fs::read(
                        fixture(name)
                            .parent()
                            .unwrap()
                            .join("expected")
                            .join(format!("collresults_{name}.txt")),
                    )
                    .unwrap();
                    assert_eq!(p.stdout, expected, "{name} nsc={nsc} order={order}");
                    if let Some(ref oracle) = oracle {
                        assert_eq!(&p.stdout, oracle);
                    } else {
                        oracle = Some(p.stdout);
                    }
                } else {
                    assert!(String::from_utf8_lossy(&p.stderr).contains("error"));
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn inherited_collection_results_are_order_independent() {
    matrix(
        &[
            ("grouped", true),
            ("scans", true),
            ("ends", true),
            ("grouped_bad", false),
            ("scan_bad", false),
        ],
        true,
    );
}
#[test]
fn sorted_results_and_existing_set_operations() {
    matrix(
        &[
            ("keys", true),
            ("key_bad", false),
            ("bitset", true),
            ("bitset_bad", false),
            ("sortedset", true),
        ],
        true,
    );
}
#[test]
fn value_class_and_nullary_overload_execute() {
    matrix(&[("iterable_once", true), ("mkstring", true)], true);
}
#[test]
fn evidence_bearing_factories() {
    matrix(
        &[
            ("sorted_factory", true),
            ("set_factories", true),
            ("map_factories", true),
            ("factory_bad_order", false),
            ("factory_bad_result", false),
        ],
        true,
    );
}
#[test]
fn java_string_members_precede_views() {
    matrix(
        &[
            ("bytes", true),
            ("chars", true),
            ("lines_java", true),
            ("lines_scala", false),
            ("lines_next_bad", false),
            ("bad_bytes", false),
        ],
        false,
    );
}
#[test]
fn for_value_guards_execute() {
    matrix(&[("for_guard", true)], false);
}

#[test]
fn actual_gitbucket_java_patch_helper_executes() {
    let base=Path::new("/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/gitbucket");
    let source = base.join("gitbucket/src/main/java/gitbucket/core/util");
    let out = std::env::temp_dir().join(format!(
        "collection-gitbucket-java-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&out).unwrap();
    let deps = fs::read_to_string(base.join("deps.cp")).unwrap();
    let cp = format!("{}:{JAR}", deps.trim());
    let p = Command::new("javac")
        .args(["-cp", &cp, "-d"])
        .arg(&out)
        .arg(source.join("PatchUtil.java"))
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    matrix_cp(
        &[("patch", true)],
        false,
        &format!("{}:{cp}", out.display()),
    );
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn result_constrained_conversion_evidence() {
    matrix(&[("conv_result", true), ("conv_result_bad", false)], false);
}

#[test]
fn sorted_map_overloads_after_generic_collection_loading() {
    matrix_ordered(
        &[
            ("treemap_map", true),
            ("sortedmap_map", true),
            ("sortedmap_flatmap", true),
            ("sortedmap_collect", true),
        ],
        Some("mapwarm"),
        JAR,
    );
}
#[test]
fn lazy_class_tag_factory_preserves_required_evidence() {
    matrix(
        &[
            ("class_tag_factory", true),
            ("class_tag_generic", true),
            ("factory_bad_tag", false),
        ],
        true,
    );
}
