# scala-rs

A compiler for a subset of Scala 2.13 (nsc), written in Rust. It reads Scala
sources and emits JVM class files.

It is not a port of scalac's sources. It is an original reimplementation.
Scala 3 syntax and TASTy are out of scope.

## Status

Experimental. This is a subset compiler, not a finished Scala compiler, and it
makes no claim of conformance to the language specification. What exists today:

- The front end carries an AST close to nsc's `Tree`: namer, typer (including
  implicit search), uncurry, lambda-lift and erasure.
- The target is Java 8 class files (major version 52), with a `StackMapTable`
  (`full_frame`) in the `Code` attribute. Frame types for locals are the erasure
  of the slot's declared type, as in scalac.
- Two ABI modes. By default the compiler links against a real
  `scala-library-2.13.x` jar when it can find one; with `--no-scala-library` it
  emits its own private runtime class files (`scala/Option`, `scala/List`,
  `scala/FunctionN`, …) instead. See [Library modes](#library-modes).
- Lambdas are emitted as `invokedynamic` through `LambdaMetafactory`, like nsc
  2.13. `PartialFunction` literals and arities above 22 are still compiled to
  anonymous classes.

直接自己末尾呼び出しは `final` / `private` / object / ローカル def でループ化します。
`@tailrec` の未対応形状は診断します。対応範囲と深い再帰・scalac 相互運用テストは
[docs/tailrec.md](docs/tailrec.md) を参照してください。

`Singleton` の型境界は ScalaSignature に保存し、scalac 2.13.16 との
分割コンパイルを両方向で検査しています。正常な継承の JVM 実行と、不正な
境界変更の拒否を確認するテストは `singleton_metadata` です。
[修正の範囲と検証方法](docs/notes/singleton-metadata-parent.md) を参照してください。
外部 trait の `hashCode` / `toString` と組み込み `Any` の上書き関係も、
[scalac 相互運用テスト](docs/notes/inherited-universal-override.md) で検査します。

同名メソッドと object の `apply` は両方をオーバーロード候補として比較します。
単一候補の呼び出し結果と曖昧な呼び出しの拒否は、`overload_module` テストで
scalac 2.13.16 と比較します。集合の結合も含む
[修正範囲と検証方法](docs/notes/overload-module-and-set.md) を参照してください。

出力先をクラスパスに含めて object を再コンパイルする場合も、Java 起動用の
`main` 転送メソッドを再生成します。`incremental_forwarder` テストは同じ出力先への
2 回のコンパイルと JVM 実行を scalac と比較し、両方向にコンパイラを入れ替えた
組み合わせも検査します。

暗黙検索は、候補を単一化する前に nsc の `isPlausiblyCompatible` にあたる構造的な
前判定（`Typer::plausibly_inhabits`）で絞り込みます。候補の結果型と要求型が
どちらもクラスで、どちらの親も他方に到達しない場合だけ棄却するので、最後の
適合判定が必ず失敗する候補しか落としません。あわせて、単一化がワイルドカードに
しか解けなかった型引数は「確定した」とは数えません（slick の `tupleNShape` のような
導出規則が、要求型が何も言っていない場面で自分の暗黙引数を探し続けるのを止めます）。
どちらも診断結果を変えず、コストだけを下げます。検証は `implfilter` テストと
[docs/gitbucket.md](docs/gitbucket.md) を参照してください。

ワイルドカード import は、まだ pickle を読んでいないクラスに問い合わせません。
問い合わせると `PickleSupply` が拒否を恒久的に記憶してしまい、直後に同じクラスを
読み込む `adopt_binary_class` がその記憶を受け取るため、jar 側の `implicit def` が
「implicit と書かれていない普通のメソッド」として暗黙スコープに残り、決して選ばれ
なくなります（バイトコードに implicit は記録されません）。スキップしたクラスは、
別の理由で読み込まれたあとの同じ import の再走査で改めて供給されます。
scalac がコンパイルした jar に対する検証は `implguard` テスト
（`tests/multi/implicit_wildcard_binary`）を参照してください。

パス依存型メンバーは高階のもの（`trait P[M[_]] { type F[_] }` の `P.F`）も
接頭部ごとに区別します。別々の `p` / `q` の `F` は別の型構成子であり、宣言が
持たない名前は従来どおり診断します。呼び出し側が書かなかった暗黙引数節でも
依存メソッド型の置換を行い、型ラムダ（kind-projector の `*` を含む）の本体に
現れる出現まで書き換えます。正常系の JVM 実行と不正例の拒否は `hkpath` テスト
（`tests/fixtures/hkp_member*.scala`）で scalac 2.13.16 と比較します。詳細は
[docs/cats.md](docs/cats.md) の「The same member, higher-kinded」を参照して
ください。

For what the language subset does and does not cover, see
[docs/language-support.md](docs/language-support.md) and
[docs/not-implemented.md](docs/not-implemented.md).

## Benchmark

The reference workload is [slick](https://github.com/slick/slick): 184 files,
23,337 lines, compiled with `-Xsource:3` against scala-library 2.13.16 plus
slick's dependency jars. Both compilers were measured on the same machine under
the same conditions, back to back.

|                | wall    | CPU     | class files |
| -------------- | ------- | ------- | ----------- |
| scalac 2.13.16 | 12.0 s  | 68.6 s  | 1498        |
| scala-rs       | 1.8 s   | 1.7 s   | 2127        |

Medians of three runs, alternating between the two compilers so that both see
the same machine. The CPU-time gap is the larger one: scalac's wall time is
carried by several threads, while scala-rs runs the compile itself on one
thread and only parallelises writing the class files.

scala-rs emits more class files than scalac for the same sources, so the
comparison is not entirely in its favour: `PartialFunction` literals still
become anonymous classes here.

What that run establishes:

- All 184 slick files typecheck, with 0 errors.
- All 1552 emitted class files load under `java -Xverify:all`. They *load*:
  `Class.forName(initialize = false)` links nothing, so method bodies are not
  verified by that number (see `tests/slick_run.sh`).
- The test suite is 130 test binaries / 1849 tests. 84 of them (the programs in
  `tests/conform/`) are dual-run against real scalac 2.13.16 and required to
  produce byte-identical stdout.

This is one benchmark, not a completeness claim. A large real program compiling
and verifying says nothing about the parts of the specification it happens not
to use.

Methodology, phase breakdown and profiling notes are in
[docs/performance.md](docs/performance.md).

## Build

A Cargo workspace. The CLI crate is `scala-rs-cli`; the binary is `scala-rs`.

```bash
cargo build -p scala-rs-cli --release
```

Or run it straight from the workspace:

```bash
cargo run -p scala-rs-cli -- compile file.scala -d out/
```

The binary lands in `target/release/scala-rs` (or `target/debug/scala-rs`).

## Usage

Compile sources into a directory of class files:

```bash
scala-rs compile file.scala -d out/
scala-rs compile file.scala -d out/ --scala-library /path/to/scala-library-2.13.16.jar
scala-rs compile file.scala -d out/ --no-scala-library
scala-rs compile B.scala -d outB -cp outA --no-scala-library
scala-rs compile file.scala -d out/ -Xsource:3
```

Compile and run the entry point (`main` in an `object Main`). `run` adds the
library jar to `java -cp` when one is in use:

```bash
scala-rs run file.scala
scala-rs run file.scala --scala-library /path/to/scala-library-2.13.16.jar
scala-rs run file.scala -- arg1 arg2
```

The emitted class files are launched by `java` exactly as scalac's are: an
`object` produces a module class `Main$` plus a forwarder `Main` carrying the
static `main`.

```bash
java -cp out Main
java -cp out:scala-library-2.13.16.jar Main
```

### Library modes

`--scala-library [<jar>]` (or the `SCALA_LIBRARY_JAR` environment variable)
links against the **scala-library 2.13 ABI**: `Option`, `List`, `FunctionN`,
`Tuple2`, `Predef$`, the `Rich*` / `StringOps` / `ArrayOps` extension methods,
the collections, `Either`, `scala.util.Try`, `scala.jdk.CollectionConverters`,
and so on come from the jar, and no colliding private class file is emitted.
Members that are not in the hand-written prelude are supplied on demand by
reading the `ScalaSignature` pickle out of the jar's class files. If the path is
omitted, `SCALA_LIBRARY_JAR`, `/tmp/scala-rs-lib` and the current directory are
searched.

`compile` and `run` use an auto-detected jar by default and fall back to the
private runtime when there is none. `--no-scala-library` forces the private
runtime.

### Debug and diagnostic flags

- `--parse` — parse only and dump the AST (no typechecking, no output).
- `--typer` — dump the tree after namer/typer. This is a dump flag, not a stop
  flag: the compile still runs to the end.
- `-Xfatal-warnings` — turn warnings (non-exhaustive match, …) into errors.
- `-Xsource:<version>` — source level: `2.13` (default), `3` or `3-cross`. The
  `3` levels accept the Scala 3 spellings this subset implements (`A & B`
  intersection types). As in nsc, a level below the current major is an error.
- `-language:<feat>` — enable `postfixOps`, `implicitConversions` or `dynamics`.
- `-cp` / `--class-path` — read previously compiled class files, Scala classes
  in jars (through their `ScalaSignature` pickle) and Java `.class` files from
  jars, jmods and the JDK.
- `SCALA_RS_PICKLE_DEBUG=1` — trace which library members were supplied from a
  pickle, and why the others were not.

`scala-rs --help` prints the full list.

## Project layout

| crate                | role                                                                    |
| -------------------- | ----------------------------------------------------------------------- |
| `crates/span`        | source positions and diagnostics                                        |
| `crates/lexer`       | lexing (newline tokens for semicolon inference, interpolation modes)     |
| `crates/parser`      | recursive-descent parser; AST close to nsc's `Tree`                     |
| `crates/pickle`      | reader for nsc `ScalaSignature` pickles, shared by typer and backend     |
| `crates/typer`       | namer, typer, implicit search, uncurry, lambda-lift, erasure             |
| `crates/backend`     | JVM class file emission (major 52, `StackMapTable`) and the private runtime |
| `crates/driver`      | pipeline driver                                                         |
| `crates/cli`         | command line; binary `scala-rs`                                         |

Test data lives outside the crates: `tests/fixtures/` (single-file programs plus
their expected stdout), `tests/multi/` and `tests/conform_multi/` (multi-file
programs), `tests/conform/` (the differential conformance corpus).

## Testing

```bash
cargo test
```

Tests that need external artifacts skip themselves rather than fail when the
artifact is missing:

- Anything comparing against real scalac needs `scalac` 2.13.16 on `PATH`.
- Anything in library mode needs `scala-library-2.13.16.jar`, found through
  `SCALA_LIBRARY_JAR` or `/tmp/scala-rs-lib`.
- The Java-side checks need `java` (and, for a few of them, `jar` and `javap`).

The scripts under `tests/` are measurement harnesses, not part of `cargo test`:

- `tests/bench.sh` — time a full compile of slick's 184 sources and report wall
  and CPU time. `--parse` times parsing only; `REPS=n` repeats.
- `tests/slick_measure.sh` — compile slick's sources and report the error count
  (correctness, not speed). It rebuilds its own toolchain and re-clones slick at
  a pinned revision when pieces are missing.
- `tests/slick_subset.sh` — find the fixpoint of slick files that compile
  cleanly together, emit their class files, and load every one of them with the
  bytecode verifier on.
- `tests/verify_all.sh <dir> [cp...]` — load every class file under a directory
  with `Class.forName(name, true, loader)` and count `VerifyError` /
  `ClassFormatError`. Initialising is the point: it forces linking, and linking
  is what runs the verifier over the method bodies. `slick_subset.sh` passes
  `false` there, so it never verifies a body; this is the check that noticed
  six of slick's 1490 classes could not be loaded at all while every other
  measure was green.
- `tests/slick_run.sh` — build slick twice (scala-rs and real scalac), compile
  the client programs in `tests/slick_progs/` once with real scalac, and run
  that one client binary against each slick build, comparing stdout byte for
  byte. The first harness that asks whether the emitted slick *runs*. Each
  program is executed `RUNS` times (default 3) and the per-program `m/n` is
  printed, so an intermittent failure cannot be averaged away. See
  [docs/notes/running-the-slick-we-compiled.md](docs/notes/running-the-slick-we-compiled.md).
- `tests/expand_fm.py` — expand the seven FreeMarker templates slick's build
  generates Scala sources from, so a measurement covers what sbt would compile.
- `tests/testkit_measure.sh` — the same measurement for `slick-testkit`, slick's
  own test suite, compiled against the class files scala-rs produced for slick.
- `tests/reap_strays.sh` — kill `scala-rs` processes orphaned by a killed test
  run (`--kill`; without it, only reports).

How the fixtures, the dual-run harnesses and the pickle-reader regression tests
are organised is described in [docs/testing.md](docs/testing.md).

別コンパイルでは型パラメータの上下限と型エイリアスの宣言種別を
`ScalaSignature` に保持します。`crates/cli/tests/existential.rs` は実際の
scalac を読み手にして、正例の JVM 実行と不正な型引数の拒否を検証します。
`E#Elem` の接頭部や curried メソッドの引数節には未解決の制限があり、
Slick の逆方向テスト（MODE=a）はまだ通っていません。
現在の数値と検証範囲は [tests/BASELINE.md](tests/BASELINE.md) を参照してください。

## Documentation

- [docs/language-support.md](docs/language-support.md) — the implemented
  language subset, feature by feature.
- [docs/not-implemented.md](docs/not-implemented.md) — what is knowingly
  missing.
- [docs/architecture.md](docs/architecture.md) — crate structure, and how
  library symbols are supplied from `ScalaSignature` pickles.
- [docs/performance.md](docs/performance.md) — benchmark methodology, phase
  breakdown, and the optimisations behind the numbers above.
- [docs/testing.md](docs/testing.md) — test layout and what each suite fixes.
- [docs/comparison-with-scalac.md](docs/comparison-with-scalac.md) — an honest
  diff against scalac 2.13.
- [docs/macros.md](docs/macros.md) — design notes for def macros, and
  [docs/macro-engine-prototype/](docs/macro-engine-prototype/) — the
  feasibility probe behind them (not production code).
- [docs/slick-testkit.md](docs/slick-testkit.md) — compiling slick's own test
  suite: what is measured, the numbers, and what they found.
- [docs/cats.md](docs/cats.md) — where this compiler stands on typelevel/cats,
  the second real-world benchmark.
  引数なしメソッドの戻り値へ挿入する `apply` でも型引数を推論し、
  レシーバの型で具体化した上限・下限を検証します（`catstail3` の実行・拒否テスト）。
- [docs/scala-library.md](docs/scala-library.md) — compiling scala/scala's own
  `src/library` from source: the numbers, and the one root behind them.
- [docs/specialization.md](docs/specialization.md) — `@specialized` はメソッドが
  所有する1型引数の Int/Long 特殊化を実装しています（object・final・private
  メソッド）。クラス特殊化は未完了です。`tests/spec_classfiles.sh` で
  scalac との差を引き続き計測します。
- [docs/notes/](docs/notes/README.md) — development notes: the investigations
  and the reasoning behind individual changes.

## License

Apache-2.0
