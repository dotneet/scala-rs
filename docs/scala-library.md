# The Scala standard library

Where this compiler stands on **its own standard library**: `src/library` of
[scala/scala](https://github.com/scala/scala), the sources the
`scala-library-2.13.16.jar` we link against is built from. Like
[cats.md](cats.md) and [gitbucket.md](gitbucket.md) this is a survey — the
point is to have the number and the known gaps written down.

This one is different from the other benchmarks in kind, not only in size.
slick, cats and gitbucket are *users* of the standard library. `src/library`
**is** the standard library, and this compiler does not learn the standard
library from source: it has it built in, as the `crates/typer/src/prelude*.rs`
signature tables (and, in `--scala-library` mode, the jar). So compiling
`src/library` asks the compiler to typecheck source definitions of the very
names it already believes it knows, and source definitions have to replace the
built-in prelude's where both exist.

## The material

| | |
|---|---|
| Repository | `https://github.com/scala/scala` |
| Revision | **`3f6bdaeafde17d790023cc3f299b81eaaf876ca3`** — tag `v2.13.16`, the same release as the jar the rest of the suite links against |
| Module | the `library` subproject's `Compile` configuration |
| Sources | **538 `.scala`** under `src/library`, plus 32 `.java` |
| Not compiled | `src/library-aux` (`Any`, `AnyRef`, `Nothing`, `Null`, `Singleton`) — `build.sbt` passes it to scaladoc as `-doc-no-compile`; those five are compiler built-ins |

The 32 Java sources (`BoxesRunTime`, `Statics`, the `*Ref` boxes, `BoxedUnit`,
`ScalaNumber`, the concurrent TrieMap bases, `ScalaSignature`) are javac's in
the real build. There is no Java front end here, so `tests/scalalib_measure.sh`
puts the 33 classfiles they produce — extracted from the released jar — on the
classpath instead.

### Flags

`build.sbt` gives the library project:

```
-feature -Xlint -sourcepath <src/library>
-Wconf:cat=unchecked&msg=The outer reference…:s -Wconf:cat=optimizer:is
-Wconf:cat=unused-nowarn:s -Wunnamed-boolean-literal-strict
```

plus `-Werror` when `fatalWarnings` is on (CI and release builds, not local
development). **None of these changes what is accepted**, so the measurement
passes none of them. In particular there is no `-Xsource:3`, no `-Yrecursion`
and no `-opt`: the optimiser is turned on only for the `bench` subproject and
for the bootstrap (`project/ScriptCommands.scala`). The one flag the
measurement does pass is nsc's own `-no-specialization`; see
[scala-corpus.md](scala-corpus.md#why-pos-does-not-pass--no-specialization).

### The measurement is not run against the jar

Linking `src/library` against `scala-library-2.13.16.jar` asks the compiler to
typecheck definitions of the classes that jar already contains, and every one
of them then reports a duplicate (`type mismatch; found: <overload None$ |
None$>`). The default mode is therefore `--no-scala-library` with only the
*Java* half of the library on the classpath, which is the arrangement that
would actually retire the jar. `SCALALIB_MODE=jar` measures the other one.

`classes=0` is expected while errors remain — nothing is emitted — but read
`errors` and `classes` together: `errors=0 classes=0` would mean a crash, not a
success.

## Running it

```
SCALALIB_LOG=$MYDIR/measure.txt SCALALIB_RUN=$MYDIR/run \
  tests/scalalib_measure.sh
```

The script passes `-no-specialization` itself; extra arguments go to
scala-rs. `SCALALIB_LOG` and `SCALALIB_RUN` default to per-invocation paths
under `SCALA_RS_FIXTURE_ROOT`; set them when you want to keep the output.
`SCALALIB_MODE=jar` switches to `--scala-library`, and `SCALALIB_DIRS` picks a
different source set. The script clones scala/scala at the pinned revision and
rebuilds the Java classpath whenever either is missing.

### Probing inside the library

Several standard-library roots reproduce only when the run's own sources
supply the class involved; a reduction written outside the library does not
see them. `tests/scalalib_probe.sh` compiles a **writable copy** of the
library so that a probe can be written into a library file:

```
tests/scalalib_probe.sh init            # make/refresh the writable copy
tests/scalalib_probe.sh [tag]           # compile it, print errors
tests/scalalib_probe.sh add p.scala     # compile it plus one extra file,
                                        # printing only that file's errors
```

Inserting `val dbg: Nothing = <expr>` into a library method makes the
compiler print what it inferred for `<expr>` as the `found:` half of the
mismatch. The probe is a debugging tool, not a gate step; run
`tests/scalalib_measure.sh` once first so the pristine sources and Java
classfiles exist.

## Where we stand

The accepted figure is in `tests/BASELINE.md`: at gate `ff08907d`
(2026-09-14) the measurement is **538 files, 2 errors in 2 files**, and
`tests/verify_merge.sh` requires exactly the errors and files recorded there.
Both errors are one unimplemented feature, not a wrong answer: a local
`object` that captures (`object partitioner` in
`RedBlackTree.partitionEntries` and `object sub` in `TreeSet.removedAll`).

With those two definitions hoisted out of their methods by hand in a writable
copy, the library compiled with 0 errors to 2614 class files that pass
`tests/classfile_lint.py`, and real scalac 2.13.16 compiled a client against
those classes by reading our pickles. Type checking is therefore green;
running the emitted library is not yet.

## Known gaps

* **A local `object` that captures** — the two remaining errors. nsc lowers it
  to a `scala.runtime.LazyRef` local; `crates/typer/src/localobj.rs` refuses
  it for now. See [not-implemented.md](not-implemented.md).
* **Scala repeated parameters forwarded to a Java varargs method**
  (`s.format(args: _*)`) are passed as the `Seq` itself, with no `Object[]`
  conversion. Inside the emitted library this is a `VerifyError` in
  `StringOps.format$extension`, and it is the first thing a client of the
  emitted library hits. See [not-implemented.md](not-implemented.md).
* In the hoisted build, 46 lambda / `$anonfun` class files were written in the
  default package with the package spelled into their names with `$`
  (`scala$PartialFunction$$$anonfun$3`); not yet diagnosed when last recorded.

## History

Earlier versions of this page recorded the survey from 4203 errors down to 2,
one section per slice (`agent/preludeshadow`, `agent/tuplelit`,
`agent/libprelude`, `agent/libzero`, `agent/libfinal` and others), with their
reductions and before/after measurements. Code comments and tests that cite
those sections (for example "`docs/scala-library.md`'s item 0" or "the
reproduction from `docs/scala-library.md`") refer to that record; read it with
`git log -p -- docs/scala-library.md`.
