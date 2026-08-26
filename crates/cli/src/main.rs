//! scala-rs command-line compiler (Scala 2.13 subset, not Scala 3).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use scala_rs_driver::{compile_paths, run_main, CompileOptions, CompileResult};
use scala_rs_span::render_all;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return ExitCode::from(1);
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
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

fn print_help() {
    println!(
        "\
scala-rs — a Scala 2.13 subset compiler (not Scala 3)

USAGE:
    scala-rs compile <files...> [-d <dir>] [--parse] [--typer]
    scala-rs run <file> [--] [java-args...]
    scala-rs --help

This is an experimental reimplementation of a Scala 2.13 (nsc) subset.
Scala 3 syntax and TASTy are not supported.

COMMANDS:
    compile    Compile Scala sources to JVM class files
    run        Compile a file to a temp directory and run its main method

OPTIONS:
    -d <dir>   Output directory for class files (default: .)
    --parse    Parse only and dump the AST (do not typecheck or emit)
    --typer    Dump the typed tree after namer/typer
    --help     Show this help

EXAMPLES:
    scala-rs compile Main.scala -d out
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
}

fn parse_compile_args(args: &[String]) -> Result<CompileArgs, String> {
    let mut out_dir = PathBuf::from(".");
    let mut parse_only = false;
    let mut typer_dump = false;
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
        } else if a.starts_with('-') {
            return Err(format!("unknown option '{a}'"));
        } else {
            files.push(PathBuf::from(a));
        }
        i += 1;
    }
    Ok(CompileArgs {
        files,
        opts: CompileOptions {
            out_dir,
            parse_only,
            typer_dump,
        },
    })
}

fn cmd_run(args: &[String]) -> ExitCode {
    let (file, java_args) = match parse_run_args(args) {
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

    let opts = CompileOptions {
        out_dir: out_dir.clone(),
        parse_only: false,
        typer_dump: false,
    };
    let result = compile_paths(&[file], &opts);
    print_diags(&result);
    if !result.ok() {
        let _ = std::fs::remove_dir_all(&out_dir);
        return ExitCode::from(1);
    }

    let main = result.mains.first().map(String::as_str).unwrap_or("Main");

    let output = match run_main(&out_dir, main, &java_args) {
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

fn parse_run_args(args: &[String]) -> Result<(PathBuf, Vec<String>), String> {
    let mut file: Option<PathBuf> = None;
    let mut java_args = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            java_args.extend_from_slice(&args[i + 1..]);
            break;
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
    Ok((file, java_args))
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
