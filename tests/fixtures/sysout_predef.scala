// A qualified `println` is a call on its own receiver, not on `Predef`.
//
// `gen_apply` used to take the print intrinsic on the strength of the *name*
// alone (`fun.name() == Some("println")`), and both emitters discard the
// qualifier: `gen_predef_println` loads `scala/Predef$.MODULE$`,
// `gen_println` loads `java/lang/System.out`. So every selection spelled
// `println` was rewritten into a call on `Predef`, whatever the program wrote.
//
// This file is *run*, because that is the only way either symptom shows.
//
//  1. `java.lang.System.err.println(x)` came out as `Predef.println(x)` and
//     printed to **stdout**. It compiles and it passes `-Xverify:all`; only
//     looking at which stream the text arrived on says otherwise.
//  2. In `--scala-library` mode a program whose own sources define
//     `scala.Predef` -- what compiling scala/scala's `src/library` does --
//     emits its own `scala/Predef$.class`, which shadows the jar's. The
//     hijacked call then dies at run time with
//     `NoSuchMethodError: 'void scala.Predef$.println(java.lang.Object)'`.
//     That is defect 1 of the `agent/libprelude` section of
//     `docs/scala-library.md`.
//
// The two halves have to be in one program, because the fix must not swing
// the other way either: the *unqualified* `println` below has to keep
// resolving through the source `Predef`, which is what
// `crates/typer/src/predef_reimport.rs` put in scope, and it must call that
// object's own method -- the `P:` prefix is how the expected output tells the
// source `Predef` apart from the jar's.
//
// `type String = java.lang.String` is not decoration: without it real scalac
// 2.13.16 refuses the file ("Symbol 'type scala.Predef.String' is missing from
// the classpath"), and this fixture is dual-run against real scalac.
package scala {

  object Predef {
    type String = java.lang.String

    def println(x: Any): Unit = {
      java.lang.System.out.println("P:" + x.toString)
    }

    def print(x: Any): Unit = {
      java.lang.System.out.print("p:" + x.toString)
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    // Unqualified: the source `scala.Predef`'s own members, reached through
    // the `Predef._` auto-import. Prefixed, so the jar's cannot pass for them.
    println("unqualified")
    print("unqualified")
    println("")

    // Qualified, on a receiver that is a `java.io.PrintStream` expression and
    // has nothing to do with `Predef`.
    java.lang.System.out.println("qualified-out")
    java.lang.System.out.print("qualified-out-print")
    java.lang.System.out.println()

    // Same, on the *other* stream. This one is invisible to every check in
    // this repository except reading stdout and stderr apart.
    java.lang.System.err.println("qualified-err")

    // Through a local binding, so the receiver is not even a constant path.
    val ps = java.lang.System.out
    ps.println("through-a-val")
  }
}
