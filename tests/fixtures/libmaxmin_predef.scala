// `Predef._` is an import, not a snapshot.
//
// nsc opens `Predef._` around every compilation unit, and `Predef` there means
// whatever `scala.Predef` resolves to. When the run's own sources define
// `scala.Predef` -- which is what compiling scala/scala's `src/library` does --
// the members of *that* object are the ones in scope. This compiler modelled
// the import by copying the prelude's `Predef` members into the base scope at
// install time, before any source was read, so a source `scala.Predef`
// contributed nothing: `tests/scalalib_measure.sh` reported 34 `value max is
// not a member of Int` and 21 `value min`, because `max` reaches `Int` only
// through `Predef.intWrapper`, which the library declares on a superclass of
// its own `Predef`.
//
// This is that shape, small enough to run. `probeWrapper` is *inherited* by
// `Predef` from `LowPriorityProbe`, exactly as `intWrapper` is inherited from
// `LowPriorityImplicits`, and it is used from a file that neither writes
// `package scala` nor imports anything.
//
// It is run and not merely compiled for a second reason. An inherited member's
// owner is a plain class, so the call has to be emitted on `scala.Predef$`, not
// on `this`: recording the members without recording the import that carried
// them typechecked and then died with `class Main$ cannot be cast to class
// scala.LowPriorityProbe`. Only running it says so.
//
// `type String = java.lang.String` is not decoration: without it real scalac
// 2.13.16 refuses the file ("Symbol 'type scala.Predef.String' is missing from
// the classpath", required by `scala.Byte.$plus`), and this fixture is
// dual-run against real scalac.
package scala {

  final class RichIntProbe(val self: Int) {
    def bigger(that: Int): Int = if (self > that) self else that
    def smaller(that: Int): Int = if (self < that) self else that
  }

  private[scala] abstract class LowPriorityProbe {
    implicit def probeWrapper(x: Int): RichIntProbe = new RichIntProbe(x)
  }

  object Predef extends LowPriorityProbe {
    type String = java.lang.String
    // `append` rather than `java.lang.System.out.println`, because codegen
    // used to rewrite *any* call named `println` into `scala.Predef$.println`
    // whatever its receiver, so a `Predef` defining its own recursed until the
    // stack ran out. That was a separate defect of the `println` intrinsic,
    // not of the import, and `agent/sysout` has since fixed it
    // (`gen_expr::unresolved_print`, `crates/cli/tests/sysout.rs`). The
    // spelling is left as it was so this fixture keeps measuring one thing.
    def println(x: Any): Unit = {
      java.lang.System.out.append(x.toString)
      java.lang.System.out.append("\n")
      ()
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    // Infix and ordinary selection, both through the inherited conversion.
    println(3 bigger 7)
    println(3 smaller 7)
    println(9.bigger(2))
    // The conversion still applies to a value whose type is only known here.
    val n: Int = 4
    println(n bigger 11)
  }
}
