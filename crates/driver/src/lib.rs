//! Compiler driver: parse → namer/typer → emit → write class files.

use std::path::{Path, PathBuf};
use std::process::Command;

use scala_rs_backend::{emit, emit_runtime};
use scala_rs_parser::{dump_tree, parse_file, Tree};
use scala_rs_span::{render_all, Diagnostic, Level, SourceFile, Span};
use scala_rs_typer::{erase, find_mains, typecheck, SymbolTable};

pub use scala_rs_backend::EmittedClass;

/// Options for [`compile_paths`].
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Directory class files are written to.
    pub out_dir: PathBuf,
    /// `--parse`: dump the AST and skip typechecking / emit.
    pub parse_only: bool,
    /// `--typer`: dump the typed tree after typechecking.
    pub typer_dump: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            out_dir: PathBuf::from("."),
            parse_only: false,
            typer_dump: false,
        }
    }
}

/// Result of compiling one or more source files.
pub struct CompileResult {
    pub diags: Vec<Diagnostic>,
    pub sources: Vec<SourceFile>,
    pub emitted: Vec<EmittedClass>,
    /// Simple object names that define `main`, e.g. `"Main"`.
    pub mains: Vec<String>,
}

impl CompileResult {
    /// True when there are no error-level diagnostics.
    pub fn ok(&self) -> bool {
        !has_errors(&self.diags)
    }

    /// Render all diagnostics against the collected source files.
    pub fn render_diags(&self) -> String {
        render_all(&self.diags, &self.sources)
    }
}

struct Unit {
    file_index: usize,
    tree: Tree,
    st: Option<SymbolTable>,
}

fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == Level::Error)
}

fn source_file_name(sf: &SourceFile) -> &str {
    sf.path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(sf.name.as_str())
}

fn dump_unit(source: &SourceFile, tree: &Tree) {
    println!("// {}", source.name);
    let dump = dump_tree(tree);
    print!("{dump}");
    if !dump.ends_with('\n') {
        println!();
    }
}

fn failed_result(diags: Vec<Diagnostic>, sources: Vec<SourceFile>) -> CompileResult {
    CompileResult {
        diags,
        sources,
        emitted: Vec::new(),
        mains: Vec::new(),
    }
}

/// Load, parse, typecheck, and emit each path.
///
/// Files are parsed first. Any parse/load error stops the pipeline (no typer,
/// no emit). Typechecking is sequential and per-file for v1 (namers do not see
/// other compilation units). Class files are written to `opts.out_dir` on
/// success unless `parse_only` is set.
pub fn compile_paths(files: &[PathBuf], opts: &CompileOptions) -> CompileResult {
    let mut diags = Vec::new();
    let mut sources = Vec::new();
    let mut units = Vec::new();

    for path in files {
        let file_index = sources.len();
        match SourceFile::load(path) {
            Ok(sf) => {
                let parsed = parse_file(&sf, file_index);
                diags.extend(parsed.diags);
                units.push(Unit {
                    file_index,
                    tree: parsed.tree,
                    st: None,
                });
                sources.push(sf);
            }
            Err(e) => {
                let name = path.display().to_string();
                diags.push(Diagnostic::error(
                    file_index,
                    Span::DUMMY,
                    format!("cannot read {}: {e}", path.display()),
                ));
                sources.push(SourceFile::from_path(path.clone(), name, String::new()));
            }
        }
    }

    if has_errors(&diags) {
        return failed_result(diags, sources);
    }

    if opts.parse_only {
        for u in &units {
            dump_unit(&sources[u.file_index], &u.tree);
        }
        return CompileResult {
            diags,
            sources,
            emitted: Vec::new(),
            mains: Vec::new(),
        };
    }

    let mut mains = Vec::new();
    for u in &mut units {
        let (mut st, tdiags) = typecheck(&mut u.tree, u.file_index);
        diags.extend(tdiags);
        mains.extend(find_mains(&st, &u.tree));
        if !has_errors(&diags) {
            erase(&mut u.tree, &mut st);
        }
        u.st = Some(st);
    }

    if opts.typer_dump {
        for u in &units {
            dump_unit(&sources[u.file_index], &u.tree);
        }
    }

    if has_errors(&diags) {
        return CompileResult {
            diags,
            sources,
            emitted: Vec::new(),
            mains,
        };
    }

    let mut emitted = emit_runtime();
    for u in &units {
        let st = u.st.as_ref().expect("unit is typed");
        let src_name = source_file_name(&sources[u.file_index]);
        emitted.extend(emit(&u.tree, st, src_name));
    }

    if let Err(e) = write_emitted(&emitted, &opts.out_dir) {
        diags.push(Diagnostic::error(
            0,
            Span::DUMMY,
            format!(
                "cannot write class files to {}: {e}",
                opts.out_dir.display()
            ),
        ));
    }

    CompileResult {
        diags,
        sources,
        emitted,
        mains,
    }
}

/// Write each class to `out_dir/{internal_name}.class`, creating package
/// subdirectories as needed (`foo/Bar` → `out_dir/foo/Bar.class`).
pub fn write_emitted(emitted: &[EmittedClass], out_dir: &Path) -> std::io::Result<()> {
    if emitted.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(out_dir)?;
    for c in emitted {
        let dest = class_path(out_dir, &c.internal_name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &c.bytes)?;
    }
    Ok(())
}

fn class_path(out_dir: &Path, internal_name: &str) -> PathBuf {
    let mut dest = out_dir.to_path_buf();
    let parts: Vec<&str> = internal_name.split('/').filter(|p| !p.is_empty()).collect();
    match parts.split_last() {
        Some((file, dirs)) => {
            for d in dirs {
                dest.push(d);
            }
            dest.push(format!("{file}.class"));
        }
        None => dest.push(".class"),
    }
    dest
}

/// Run `java -cp out_dir main_class args...`.
///
/// `main_class` may be an internal name (`foo/Bar`); slashes are converted to
/// dots for the JVM (`foo.Bar`).
pub fn run_main(
    out_dir: &Path,
    main_class: &str,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    let dotted = main_class.replace('/', ".");
    Command::new("java")
        .arg("-cp")
        .arg(out_dir)
        .arg(&dotted)
        .args(args)
        .output()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fresh_dir() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "scala-rs-driver-{}-{}-{}",
            std::process::id(),
            n,
            nanos
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }

    fn java_available() -> bool {
        Command::new("java")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn write_emitted_package_and_module_names() {
        let tmp = fresh_dir();
        let emitted = vec![
            EmittedClass {
                internal_name: "Main$".into(),
                bytes: vec![0xCA, 0xFE],
            },
            EmittedClass {
                internal_name: "foo/Bar".into(),
                bytes: vec![0xBA, 0xBE],
            },
        ];
        write_emitted(&emitted, &tmp.0).unwrap();
        let module = tmp.0.join("Main$.class");
        let pkg = tmp.0.join("foo").join("Bar.class");
        assert!(module.is_file(), "missing {}", module.display());
        assert!(pkg.is_file(), "missing {}", pkg.display());
        assert_eq!(std::fs::read(module).unwrap(), vec![0xCA, 0xFE]);
        assert_eq!(std::fs::read(pkg).unwrap(), vec![0xBA, 0xBE]);
    }

    #[test]
    fn compile_result_ok_is_false_on_errors() {
        let r = CompileResult {
            diags: vec![Diagnostic::error(0, Span::DUMMY, "boom")],
            sources: vec![],
            emitted: vec![],
            mains: vec![],
        };
        assert!(!r.ok());
        let r = CompileResult {
            diags: vec![],
            sources: vec![],
            emitted: vec![],
            mains: vec![],
        };
        assert!(r.ok());
    }

    #[test]
    fn compile_hello_snippet_and_maybe_run() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Hello.scala");
        std::fs::write(
            &src,
            r#"
object Main {
  def main(args: Array[String]): Unit = println("hello, scala-rs")
}
"#,
        )
        .unwrap();

        let opts = CompileOptions {
            out_dir: tmp.0.clone(),
            parse_only: false,
            typer_dump: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(result.ok(), "compile failed:\n{}", result.render_diags());
        assert!(
            result.mains.iter().any(|m| m == "Main"),
            "expected Main in {:?}",
            result.mains
        );

        if java_available() && !result.emitted.is_empty() {
            let main = result.mains.first().map(String::as_str).unwrap_or("Main");
            let output = run_main(&tmp.0, main, &[]).expect("run java");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "java failed: status={:?} stdout={stdout:?} stderr={stderr:?}",
                output.status
            );
            assert!(
                stdout.contains("hello, scala-rs"),
                "unexpected stdout: {stdout:?}"
            );
        }
    }

    #[test]
    fn parse_only_does_not_emit() {
        let tmp = fresh_dir();
        let src = tmp.0.join("P.scala");
        std::fs::write(
            &src,
            "object Main { def main(args: Array[String]): Unit = () }\n",
        )
        .unwrap();
        let opts = CompileOptions {
            out_dir: tmp.0.join("out"),
            parse_only: true,
            typer_dump: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(result.ok(), "{}", result.render_diags());
        assert!(result.emitted.is_empty());
        assert!(!opts.out_dir.exists());
    }

    #[test]
    fn type_error_is_not_ok() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Bad.scala");
        std::fs::write(&src, "object M { def f(): Int = foo }\n").unwrap();
        let opts = CompileOptions {
            out_dir: tmp.0.join("out"),
            parse_only: false,
            typer_dump: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(!result.ok());
        assert!(result.emitted.is_empty());
        assert!(result.diags.iter().any(|d| d.message.contains("not found")));
    }
}
