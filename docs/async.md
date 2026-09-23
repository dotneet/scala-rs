# scala-async / `-Xasync`

## Usage

Add scala-async 1.0.1 for Scala 2.13 to the class path.

```sh
target/release/scala-rs compile Main.scala \
  --scala-library /path/to/scala-library-2.13.16.jar \
  -cp /path/to/scala-async_2.13-1.0.1.jar -Xasync -d out
java -cp out:/path/to/scala-library-2.13.16.jar:/path/to/scala-async_2.13-1.0.1.jar Main
```

```scala
import scala.async.Async.{async, await}
import scala.concurrent.{Await, Future}
import scala.concurrent.ExecutionContext.Implicits.global
import scala.concurrent.duration.Duration

object Main {
  def main(args: Array[String]): Unit = {
    val answer = async {
      val first = await(Future.successful(40))
      first + await(Future.successful(2))
    }
    println(Await.result(answer, Duration(10, "seconds")))
  }
}
```

`async` returns a `Future`. `await` is a marker for suspending and resuming
asynchronous work; it is not the thread-blocking `Await.result`. The
`Await.result` in the example above is used explicitly so that `main` waits for
the output.

## Transformation and coverage

`crates/typer/src/async_lower.rs` recognizes the library calls after typing and
rewrites them into `Future.flatMap` and `Future.successful`. It keeps the
original symbols so that references to and captures of local variables are
preserved, and it saves operands that span a suspension point in temporaries.
`while` / `do-while` advance to the next iteration through a local method that
returns a Future. When an `await` inside a guard yields false, the next case is
tried.

The runtime examples are `tests/fixtures/async_runtime.scala` and
`async_values.scala`:

- No suspension, consecutive awaits, nested `async` / `await`.
- On a single-threaded execution context, code that waits on a Promise and code
  that completes it.
- `val` / `var`, assignment, array and field updates, constructor arguments.
- async inside class and object field initializers, and shared mutable local
  variables.
- `if`, `match` with asynchronous guards, short-circuit evaluation, a
  10,000-iteration loop.
- Type arguments, the common type of differing branch results, captures of local
  methods and classes.
- Failure of the awaited value, exceptions in the body, evaluating the
  ExecutionContext exactly once.
- Import aliases, and ordinary methods with the same name that are not
  transformed.
- Non-local `return`, `return await(...)`, and return targets across loops and
  generated classes.

### Non-local `return`

A `return` inside `async` targets the lexically enclosing method, as in Scala
2.13. It is not syntax for returning the result of the `async` block.
Each invocation of the target method allocates a fresh key, and only the
invocation whose key matches the `scala.runtime.NonLocalReturnControl` catches
it. When the same method is re-entered, an inner invocation does not steal the
return value.

With an ExecutionContext that runs immediately, the return can reach the outer
method while it is still running. If the outer method has already finished by
the time execution resumes after a suspension, this control exception
propagates out and the async Future stays incomplete. Do not use a general
`return` to deliver the result of asynchronous work, including through the
execution context's exception reporting.
`async_return.scala` compares immediate execution, delayed resumption,
re-entry, and `finally` against real scalac.

### Transform hook for custom libraries

Macros that call `c.internal.markForAsyncTransform(owner, method, awaitSymbol, config)`
are accepted too. The transformation targets the given await method, which does
not have to be named `await`. This path does not need the scala-async jar.
As with ordinary macros, the macro implementation's class path needs
scala-reflect.

It uses the following protocol provided by the generated class:

- `state` / `state_=`: the initial state and the state after a suspension.
- `onComplete(awaitable)`: registers the resumption callback.
- `getCompleted(awaitable)`: optional; fetches an already-completed result.
- `tryGet(completion)`: fetches the value. Returning the state machine itself
  stops the continuation.
- `completeSuccess(value)` / `completeFailure(error)`: report the result.

Internally it connects a Promise to the continuations. Pending continuations
are processed through a queue, so awaiting already-completed values repeatedly
does not grow the call stack.
The `allowExceptionsToPropagate` setting is supported as well: instead of
passing exceptions to `completeFailure`, it propagates them to the caller.
Unknown setting keys are ignored, as in nsc.
With the default settings, any `Throwable`, including control exceptions, is
passed to `completeFailure` with its original type. Propagation of a non-local
`return` in a custom library therefore also depends on how that method is
implemented. For example, an Option implementation that rethrows the exception
returns to the outer method, while a CompletableFuture implementation that
stores the exception completes the Future exceptionally.

`async_hook_impl.scala` / `async_hook_runtime.scala` compare suspension,
resumption, early exit, exceptions, and a 10,000-iteration loop using macros for
a custom Option and for Java's CompletableFuture. Differential tests also cover
1,000 completions from another thread, the interrupt status, exceptions from
completion hooks, and ordinary overloads with the same name.
`async_hook_bad.scala` rejects awaits left outside the transformed code and
invalid nesting.

`async_bad.scala` rejects `await` outside the body, inside nested methods,
functions, classes, and objects, in lazy vals, in by-name arguments, and inside
`try`. Local definitions and `try` that contain no `await` are allowed.

## Verification

The same sources are compiled with Scala 2.13.16 and the outputs are compared
under `java -Xverify:all`. Single-threaded tests run with a time limit, so a
deadlock in a blocking implementation is also detected as a test failure. When
the jar is not in the usual Coursier cache:

```sh
SCALA_ASYNC_JAR=/path/to/scala-async_2.13-1.0.1.jar tests/cli_test.sh xflags
```

As a related type-checking fix, the parent types of `scala.concurrent` classes
outside the hand-written prelude are completed before argument conformance is
checked. This makes the relationships between `Future[A]` and `Awaitable[A]`,
and between `FiniteDuration` and `Duration`, available to `Await.result`
independently of load order.

## Limitations and references

The single `FutureStateMachine` that nsc generates, and its bytecode shape and
allocation counts, are not matched. The generic hook's `postAnfTransform` /
`stateDiagram` configuration callbacks, and the form that passes the state
machine instance as an extra parameter, are unsupported and diagnosed
explicitly. This does not therefore mean full compatibility with the whole
internal `-Xasync` API.

Primary sources:

- [scala-async usage and limitations](https://github.com/scala/scala-async)
- `scala/async/Async.scala` in the scala-async 1.0.1 sources jar:
  `async` is a macro and `await` is a compileTimeOnly marker; it checks
  `-Xasync` and then calls `markForAsyncTransform`.
- [Scala 2.13's public transform hook](https://github.com/scala/scala/blob/v2.13.16/src/reflect/scala/reflect/api/Internals.scala)
- [Scala 2.13's async transform](https://github.com/scala/scala/blob/v2.13.16/src/compiler/scala/tools/nsc/transform/async/AsyncPhase.scala)
- [Design of the compiler-side -Xasync](https://contributors.scala-lang.org/t/design-of-xasync/4419)

Boundaries of the differential tests: with Scala 2.13.16 / scala-async 1.0.1,
`mark("a") + await(Future { mark("b") }) + mark("c")` ran its side effects in
the order `bac`. scala-rs saves operands left to right and produces `abc`.
The shared runtime test makes the order explicit with `val first = mark("a")`.
Also, an example that, after a suspension, uses only a value captured
indirectly by a local class defined before the suspension produced a
`VerifyError` with the same reference compiler. The shared test keeps a direct
reference to that value as well. Matching the reference compiler is not claimed
for these cases.
