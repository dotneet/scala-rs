# Development notes

Working notes from building scala-rs, one bug at a time. Each chapter records a
symptom, the root cause behind it, the fix, and how it was checked against real
scalac 2.13.16 — usually by compiling the same program with both compilers and
diffing the output byte for byte.

They are kept because the root cause was almost never what the diagnostic said.
A `value map is not a member of Any` turned out to be a mixin forwarder read
from a classfile; `no matching overload` turned out to be a single candidate
rejecting its arguments. If you are chasing something similar, the useful thing
here is the shape of the reasoning, not the conclusion.

Chapter headings carry the slice they came from (`agent/xxx`).

## The type checker

| file | what it covers |
| --- | --- |
| [expected-types-and-implicit-search.md](expected-types-and-implicit-search.md) | How an expected type reaches an argument, how implicit parameter clauses get filled — and the cases where a symbol never became visible to the search at all. |
| [implicits-and-numeric-typeclasses.md](implicits-and-numeric-typeclasses.md) | Implicit search against the 2.13 type-class hierarchies: `Integral` / `Fractional`, cats-style syntax classes, `BuildFrom`. |
| [inference-and-sequence-patterns.md](inference-and-sequence-patterns.md) | What an argument or a pattern actually *is*: argument base types, auto-tupling, sequence and stable-identifier patterns. |
| [type-mismatch-and-overload-resolution.md](type-mismatch-and-overload-resolution.md) | The `type mismatch` and `no matching overload` clusters. Most were a signature modelled as monomorphic, wearing the mask of an inference bug. |
| [application-chains-and-copy.md](application-chains-and-copy.md) | Why an application chain has to be typed as a whole: `super` and self-types, bound types under `x @ Extractor(...)`, curried `copy` and `new`. |
| [array-and-collection-typing.md](array-and-collection-typing.md) | The type-checking side of `Array`, `Set` / `Map`, and collection arguments. |
| [rejection-rules-and-differential-probes.md](rejection-rules-and-differential-probes.md) | The rules that *reject* — variance, self-type conformance, wildcard bounds, lub — where a bug shows up as a false positive on legal code. Plus a round of differential probing. |

## Symbols, companions, and separate compilation

| file | what it covers |
| --- | --- |
| [companions-and-class-symbols.md](companions-and-class-symbols.md) | How class symbols and their companions are built: parents that do not exist, an `apply` loaded twice, a missing companion classfile. |
| [jar-and-pickle-symbols.md](jar-and-pickle-symbols.md) | Where a symbol's type comes from when the class lives in a jar, and what gets lost on the way. |
| [macro-reflect-and-reify.md](macro-reflect-and-reify.md) | The reflection API surface supplied from pickles, and the expansion of `reify { … }`. |
| [slick-dsl-overloads-and-macros.md](slick-dsl-overloads-and-macros.md) | slick's own API surface: `DBIOAction`, `TableQuery` / `Compiled`, and the `ShapedValue.mapTo` macro. |

## Below the typer

| file | what it covers |
| --- | --- |
| [codegen-and-stackmap-frames.md](codegen-and-stackmap-frames.md) | Six slices where the typer was happy and the classfile was not: erasure, `StackMapTable` frames, `BoxedUnit`, statements in a template body. |
| [outer-refs-arrays-and-lambda-codegen.md](outer-refs-arrays-and-lambda-codegen.md) | Reaching an enclosing class from an anonymous class or lambda, `Array` codegen, and lowering lambdas to `invokedynamic`. |
| [bytecode-and-java-interop.md](bytecode-and-java-interop.md) | Operand-stack shape around `Unit`, nested Java interfaces and interface statics, trait method access flags. |

## Process

| file | what it covers |
| --- | --- |
| [override-checking-and-fixture-inventory.md](override-checking-and-fixture-inventory.md) | Override conformance (SLS 5.1.4, 5.2.6), and an inventory of the test fixtures. |
| [known-gaps-backlog.md](known-gaps-backlog.md) | The running "Remaining" list: gaps found but deliberately left alone, with the reasoning for leaving them. |

## Related

- [../language-support.md](../language-support.md) — what the subset covers.
- [../not-implemented.md](../not-implemented.md) — what it does not.
- [../architecture.md](../architecture.md) — how the compiler is put together.
- [../macros.md](../macros.md) — the macro design, including the JVM bridge.
- [../performance.md](../performance.md) — benchmark methodology and profiling.
- [../testing.md](../testing.md) — the test suite and the differential harnesses.
