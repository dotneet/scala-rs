//! scala-rs command-line compiler (Scala 2.13 subset, not Scala 3).

/// The compiler is allocation-bound: 42% of samples in a `sample` profile of a
/// 184-file slick build were in the system allocator. macOS's libmalloc pays a
/// lock on every small allocation; mimalloc's thread-local free lists do not.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use scala_rs_driver::{
    compile_paths, find_scala_library, find_scala_xml, run_main_with_cp, CompileOptions,
    CompileResult, SourceFeatures,
};
use scala_rs_span::render_all;

fn main() -> ExitCode {
    // Deeply nested types and long method chains recurse; the default 8 MB
    // main-thread stack is not enough for a real project.
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(run)
        .expect("spawn compiler thread")
        .join()
        .unwrap_or(ExitCode::from(2))
}

fn run() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return ExitCode::from(1);
    }

    if wants_help(&args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    let cmd = args.remove(0);
    match cmd.as_str() {
        "compile" => cmd_compile(&args),
        "run" => cmd_run(&args),
        "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command '{other}'");
            eprintln!();
            print_help();
            ExitCode::from(1)
        }
    }
}

fn wants_help(args: &[String]) -> bool {
    for a in args {
        if a == "--" {
            return false;
        }
        if a == "--help" || a == "-h" {
            return true;
        }
    }
    false
}

fn print_help() {
    println!(
        "\
scala-rs — a Scala 2.13 subset compiler (not Scala 3)

USAGE:
    scala-rs compile <files...> [-d <dir>] [-cp <path>] [--scala-library <jar>] [--no-scala-library] [--parse] [--typer] [-Xfatal-warnings] [-language:<feat>] [-Xsource:3] [-Xsource-features:<features>] [-Xasync] [-no-specialization]
    scala-rs run <file> [--scala-library <jar>] [--no-scala-library] [--] [java-args...]
    scala-rs --help

This is an experimental reimplementation of a Scala 2.13 (nsc) subset.
Scala 3 syntax and TASTy are not supported.

COMMANDS:
    compile    Compile Scala sources to JVM class files
    run        Compile a file to a temp directory and run its main method

OPTIONS:
    -d <dir>   Output directory for class files (default: .)
    -cp <path> Classpath of previously compiled class files (`:`-separated)
    --class-path <path>
               Same as -cp
    --scala-library [<jar>]
               Link against scala-library 2.13 (do not emit private Option/List).
               Path optional: searches SCALA_LIBRARY_JAR, /tmp/scala-rs-lib, cwd.
               `compile` and `run` auto-use a found 2.13 jar by default.
    --no-scala-library
               Force the private runtime even if a jar is auto-found.
    --parse             Parse only and dump the AST (do not typecheck or emit)
    --typer             Dump the typed tree after namer/typer
    -Xfatal-warnings    Treat warnings as errors (non-exhaustive match, …)
    -language:<feat>    Enable a language feature (`postfixOps`, `implicitConversions`, `dynamics`)
    -Xsource:<version>  Source level: `2.13` (default), `3`, or `3-cross`.
                        `3`/`3-cross` accept the Scala 3 spellings this subset
                        implements (`A & B` compound types).
    -Xsource-features:<features>
                        Enable Scala 3 behaviours under -Xsource:3 (ignored,
                        with a warning, without it). `3-cross` is `3` plus
                        every feature. `-Xsource-features:help` lists them;
                        `case-apply-copy-access` is the one implemented here.
    -no-specialization  Ignore `@specialized` / `@unspecialized` (nsc's flag of
                        the same name). Without it they are diagnosed: there is
                        no specialisation phase here, so the emitted class would
                        silently lack the `$mc*$sp` members callers link against.
    -Xasync             Enable the async phase for scala.async.Async's `async`
                        and `await`. The state-machine transform is not
                        implemented: an `async` block is diagnosed either way.
    --help              Show this help

EXAMPLES:
    scala-rs compile Main.scala -d out
    scala-rs compile Main.scala --scala-library scala-library-2.13.16.jar -d out
    scala-rs compile Main.scala --no-scala-library -d out
    scala-rs compile Main.scala --parse
    scala-rs run Main.scala
    scala-rs run Main.scala -- arg1 arg2
"
    );
}

fn print_diags(result: &CompileResult) {
    if result.diags.is_empty() {
        return;
    }
    eprint!("{}", render_all(&result.diags, &result.sources));
}

fn cmd_compile(args: &[String]) -> ExitCode {
    let parsed = match parse_compile_args(args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(1);
        }
    };
    if parsed.features_help {
        print!("{}", SourceFeatures::help_text());
        return ExitCode::SUCCESS;
    }
    for w in &parsed.warnings {
        eprintln!("warning: {w}");
    }
    if parsed.files.is_empty() {
        eprintln!("error: no input files");
        return ExitCode::from(1);
    }

    let result = compile_paths(&parsed.files, &parsed.opts);
    print_diags(&result);
    if !result.ok() {
        return ExitCode::from(1);
    }

    if !parsed.opts.parse_only {
        let n = result.emitted.len();
        println!(
            "wrote {n} class file{} to {}",
            if n == 1 { "" } else { "s" },
            parsed.opts.out_dir.display()
        );
    }
    ExitCode::SUCCESS
}

struct CompileArgs {
    files: Vec<PathBuf>,
    opts: CompileOptions,
    /// Settings-level warnings (nsc reports these before it reads any source).
    warnings: Vec<String>,
    /// `-Xsource-features:help` was asked for; print the list and stop.
    features_help: bool,
}

fn parse_compile_args(args: &[String]) -> Result<CompileArgs, String> {
    let mut out_dir = PathBuf::from(".");
    let mut parse_only = false;
    let mut typer_dump = false;
    let mut fatal_warnings = false;
    let mut scala_library = None;
    let mut no_scala_library = false;
    let mut class_path = Vec::new();
    let mut language_features = Vec::new();
    let mut xsource3 = false;
    let mut xsource_cross = false;
    let mut named_features = SourceFeatures::default();
    let mut named_features_given = false;
    let mut unimplemented_features: Vec<&'static str> = Vec::new();
    let mut features_help = false;
    let mut xasync = false;
    let mut no_specialization = false;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            files.extend(args[i + 1..].iter().map(PathBuf::from));
            break;
        } else if a == "-d" {
            i += 1;
            let dir = args
                .get(i)
                .ok_or_else(|| "option -d requires a directory argument".to_string())?;
            out_dir = PathBuf::from(dir);
        } else if let Some(dir) = a.strip_prefix("-d") {
            if dir.is_empty() {
                return Err("option -d requires a directory argument".into());
            }
            out_dir = PathBuf::from(dir);
        } else if a == "--parse" {
            parse_only = true;
        } else if a == "--typer" {
            typer_dump = true;
        } else if a == "-Xfatal-warnings" {
            fatal_warnings = true;
        } else if a == "--no-scala-library" {
            no_scala_library = true;
        } else if a == "--scala-library" || a.starts_with("--scala-library=") {
            scala_library = Some(take_scala_library_flag(args, &mut i)?);
        } else if a == "-cp" || a == "--class-path" || a == "-classpath" {
            i += 1;
            let cp = args
                .get(i)
                .ok_or_else(|| "option -cp requires a classpath argument".to_string())?;
            class_path.extend(split_classpath(cp));
        } else if let Some(rest) = a.strip_prefix("-language:") {
            if rest.is_empty() {
                return Err("option -language: requires a feature name".into());
            }
            for feat in rest.split(',') {
                let f = feat.trim();
                if !f.is_empty() {
                    language_features.push(f.to_string());
                }
            }
        } else if let Some(rest) = a.strip_prefix("-Xsource-features:") {
            let parsed = SourceFeatures::parse(rest)?;
            named_features = parsed.features;
            named_features_given = true;
            features_help |= parsed.help;
            unimplemented_features.extend(parsed.unimplemented);
        } else if a == "-Xsource-features" {
            i += 1;
            let spec = args.get(i).ok_or_else(|| {
                "option -Xsource-features requires a feature argument".to_string()
            })?;
            let parsed = SourceFeatures::parse(spec)?;
            named_features = parsed.features;
            named_features_given = true;
            features_help |= parsed.help;
            unimplemented_features.extend(parsed.unimplemented);
        } else if a == "-Xasync" {
            xasync = true;
        } else if a == "-no-specialization" || a == "--no-specialization" {
            no_specialization = true;
        } else if let Some(rest) = a.strip_prefix("-Xsource:") {
            (xsource3, xsource_cross) = parse_xsource_level(rest)?;
        } else if a == "-Xsource" {
            i += 1;
            let ver = args
                .get(i)
                .ok_or_else(|| "option -Xsource requires a version argument".to_string())?;
            (xsource3, xsource_cross) = parse_xsource_level(ver)?;
        } else if a == "-language" {
            i += 1;
            let feats = args
                .get(i)
                .ok_or_else(|| "option -language requires a feature argument".to_string())?;
            for feat in feats.split(',') {
                let f = feat.trim();
                if !f.is_empty() {
                    language_features.push(f.to_string());
                }
            }
        } else if a.starts_with('-') {
            return Err(format!("unknown option '{a}'"));
        } else {
            files.push(PathBuf::from(a));
        }
        i += 1;
    }
    let resolved = if no_scala_library {
        None
    } else {
        match &scala_library {
            Some(p) if p.as_os_str().is_empty() => {
                Some(find_scala_library().ok_or_else(|| {
                    "could not find scala-library 2.13 jar (pass --scala-library <jar> or set SCALA_LIBRARY_JAR)".to_string()
                })?)
            }
            Some(p) => Some(p.clone()),
            None => find_scala_library(),
        }
    };
    let (source_features, warnings) = reconcile_source_features(
        xsource3,
        xsource_cross,
        named_features,
        named_features_given,
        &unimplemented_features,
    );
    Ok(CompileArgs {
        files,
        opts: CompileOptions {
            out_dir,
            parse_only,
            typer_dump,
            fatal_warnings,
            scala_library: resolved,
            class_path: class_path,
            language_features,
            xsource3,
            source_features,
            xasync,
            no_specialization,
        },
        warnings,
        features_help,
    })
}

/// `-Xsource:<version>`. Returns `(source3, cross)`: `source3` is true when
/// the level enables Scala 3 syntax, `cross` when the level is `3-cross`,
/// which nsc defines as `-Xsource:3 -Xsource-features:_` (the post-set hook of
/// `ScalaSettings.source` calls `XsourceFeatures.tryToSet(List("_"))`).
/// nsc refuses anything below the current major version.
fn parse_xsource_level(ver: &str) -> Result<(bool, bool), String> {
    match ver.trim() {
        "" => Err("option -Xsource: requires a version".into()),
        "3" => Ok((true, false)),
        "3-cross" => Ok((true, true)),
        "2.13" | "2.13.0" => Ok((false, false)),
        other => Err(format!(
            "-Xsource must be at least the current major version (2.13.0), got '{other}'"
        )),
    }
}

/// nsc's `ScalaSettings.conflictWarning`: `-Xsource-features` is gated on
/// `isScala3`, so below `-Xsource:3` the whole setting is dropped.
const XSOURCE_FEATURES_CONFLICT: &str = "Conflicting compiler settings were detected. \
Some settings will be ignored.\n-Xsource-features requires -Xsource:3";

/// Reconcile `-Xsource` with `-Xsource-features`, exactly as nsc does.
///
/// `cross` (`-Xsource:3-cross`) turns on every feature; naming features
/// without `-Xsource:3` drops them with a warning. Returns the settings that
/// survive plus the warnings to print.
fn reconcile_source_features(
    source3: bool,
    cross: bool,
    named: SourceFeatures,
    named_given: bool,
    unimplemented: &[&'static str],
) -> (SourceFeatures, Vec<String>) {
    let mut warnings = Vec::new();
    let mut features = named;
    if cross {
        features = SourceFeatures::all();
    }
    if named_given && !source3 {
        warnings.push(XSOURCE_FEATURES_CONFLICT.to_string());
        features = SourceFeatures::default();
    }
    if !features.is_empty() {
        for f in unimplemented {
            warnings.push(format!(
                "-Xsource-features:{f} is accepted but not implemented by scala-rs; \
it changes nothing (see docs/not-implemented.md)"
            ));
        }
    }
    (features, warnings)
}

fn take_scala_library_flag(args: &[String], i: &mut usize) -> Result<PathBuf, String> {
    let a = args[*i].as_str();
    if let Some(jar) = a.strip_prefix("--scala-library=") {
        if jar.is_empty() {
            return Ok(PathBuf::new());
        }
        return Ok(PathBuf::from(jar));
    }
    let next = args.get(*i + 1).map(|s| s.as_str());
    if let Some(n) = next {
        if n.ends_with(".jar") || (std::path::Path::new(n).is_file() && !n.ends_with(".scala")) {
            *i += 1;
            return Ok(PathBuf::from(n));
        }
    }
    Ok(PathBuf::new())
}

fn split_classpath(s: &str) -> Vec<PathBuf> {
    s.split(':')
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn cmd_run(args: &[String]) -> ExitCode {
    let parsed = match parse_run_args(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(1);
        }
    };

    let out_dir = match make_temp_out() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot create temp directory: {e}");
            return ExitCode::from(1);
        }
    };

    let scala_library = parsed.scala_library;
    let opts = CompileOptions {
        out_dir: out_dir.clone(),
        parse_only: false,
        typer_dump: false,
        fatal_warnings: false,
        scala_library: scala_library.clone(),
        class_path: Vec::new(),
        language_features: Vec::new(),
        xsource3: parsed.xsource3,
        source_features: parsed.source_features,
        xasync: parsed.xasync,
        no_specialization: parsed.no_specialization,
    };
    let result = compile_paths(&[parsed.file], &opts);
    print_diags(&result);
    if !result.ok() {
        let _ = std::fs::remove_dir_all(&out_dir);
        return ExitCode::from(1);
    }

    let main = result.mains.first().map(String::as_str).unwrap_or("Main");
    let mut extra: Vec<PathBuf> = scala_library.into_iter().collect();
    if let Some(xml) = find_scala_xml() {
        extra.push(xml);
    }

    let output = match run_main_with_cp(&out_dir, &extra, main, &parsed.java_args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: failed to run java: {e}");
            let _ = std::fs::remove_dir_all(&out_dir);
            return ExitCode::from(1);
        }
    };

    let _ = std::io::stdout().write_all(&output.stdout);
    let _ = std::io::stderr().write_all(&output.stderr);
    let _ = std::fs::remove_dir_all(&out_dir);

    match output.status.code() {
        Some(code) => ExitCode::from(code as u8),
        None => ExitCode::from(1),
    }
}

struct RunArgs {
    file: PathBuf,
    java_args: Vec<String>,
    scala_library: Option<PathBuf>,
    xsource3: bool,
    source_features: SourceFeatures,
    xasync: bool,
    no_specialization: bool,
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, String> {
    let mut file: Option<PathBuf> = None;
    let mut java_args = Vec::new();
    let mut scala_library = None;
    let mut no_scala_library = false;
    let mut xsource3 = false;
    let mut xsource_cross = false;
    let mut named_features = SourceFeatures::default();
    let mut named_features_given = false;
    let mut xasync = false;
    let mut no_specialization = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            java_args.extend_from_slice(&args[i + 1..]);
            break;
        } else if let Some(rest) = a.strip_prefix("-Xsource-features:") {
            named_features = SourceFeatures::parse(rest)?.features;
            named_features_given = true;
        } else if a == "-Xasync" {
            xasync = true;
        } else if a == "-no-specialization" || a == "--no-specialization" {
            no_specialization = true;
        } else if let Some(rest) = a.strip_prefix("-Xsource:") {
            (xsource3, xsource_cross) = parse_xsource_level(rest)?;
        } else if a == "--no-scala-library" {
            no_scala_library = true;
        } else if a == "--scala-library" || a.starts_with("--scala-library=") {
            scala_library = Some(take_scala_library_flag(args, &mut i)?);
        } else if file.is_none() {
            if a.starts_with('-') {
                return Err(format!("unknown option '{a}'"));
            }
            file = Some(PathBuf::from(a));
        } else {
            java_args.push(a.to_string());
        }
        i += 1;
    }
    let file = file.ok_or_else(|| "run requires a source file".to_string())?;
    let scala_library = if no_scala_library {
        None
    } else {
        match scala_library {
            Some(p) if p.as_os_str().is_empty() => {
                Some(find_scala_library().ok_or_else(|| {
                    "could not find scala-library 2.13 jar (pass --scala-library <jar> or set SCALA_LIBRARY_JAR)".to_string()
                })?)
            }
            Some(p) => Some(p),
            None => find_scala_library(),
        }
    };
    let (source_features, warnings) = reconcile_source_features(
        xsource3,
        xsource_cross,
        named_features,
        named_features_given,
        &[],
    );
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok(RunArgs {
        file,
        java_args,
        scala_library,
        source_features,
        xasync,
        xsource3,
        no_specialization,
    })
}

fn make_temp_out() -> std::io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("scala-rs-run-{}-{}", std::process::id(), nanos));
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
