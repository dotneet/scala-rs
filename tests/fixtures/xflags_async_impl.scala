// `-Xasync` is not a compiler behaviour a program can observe directly: nsc's
// setting only enables `scala.tools.nsc.transform.async`, and the message a
// user gets for a missing `-Xasync` comes from the *library*. This is
// `scala.async.Async.asyncImpl`'s own gate, verbatim (scala-async 1.0.1):
//
//   if (!c.compilerSettings.contains("-Xasync"))
//     c.abort(c.macroApplication.pos,
//       "The async requires the compiler option -Xasync (supported only by
//        Scala 2.12.12+ / 2.13.3+)")
//
// so this fixture is the part of `-Xasync` that scala-rs implements:
// `c.compilerSettings` reports the compiler's own command line, and a macro
// that gates on a flag sees the same list under both compilers.
import scala.reflect.macros.blackbox.Context

object XflagsAsyncImpl {
  def gateImpl(c: Context)(body: c.Tree): c.Tree = {
    if (!c.compilerSettings.contains("-Xasync")) {
      c.abort(
        c.macroApplication.pos,
        "The async requires the compiler option -Xasync (supported only by Scala 2.12.12+ / 2.13.3+)"
      )
    }
    body
  }
}
