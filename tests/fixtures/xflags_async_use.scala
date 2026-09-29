// The call site for `xflags_async_impl.scala`. Compiles only with `-Xasync`;
// without it the macro aborts with scala-async's own message. It expands the
// macro twice: scala-rs sends the settings with the first request only, and
// the second must still see them.
import scala.language.experimental.macros

object XflagsAsyncUse {
  def gate(body: Int): Int = macro XflagsAsyncImpl.gateImpl
}

object Main {
  def main(args: Array[String]): Unit =
    println(XflagsAsyncUse.gate(41 + 1) + XflagsAsyncUse.gate(0))
}
