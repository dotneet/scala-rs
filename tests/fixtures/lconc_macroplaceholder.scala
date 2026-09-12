// `def f(...): T = macro ???` is a *placeholder*, not an implementation. nsc's
// `scala/reflect/macros/compiler/Validators.scala` runs only the confidence
// check on it and skips `checkMacroDefMacroImplCorrespondence` (`if (macroImpl
// != Predef_???) ...`), so the definition stands; only a call site is refused.
// The library declares its compiler-intrinsic macros this way --
// `StringContext.s/f/raw` and `scala.reflect.materializeClassTag` -- and nsc
// fills them in from its own `FastTrack` table.
import scala.language.experimental.macros

object Main {
  def s(args: Any*): String = macro ???
  def f[A >: Any](args: A*): String = macro ???
  private[this] def materializeTag[T](): String = macro ???

  def main(args: Array[String]): Unit = {
    // The definitions compile and no bytecode is emitted for them; nothing
    // calls them, exactly as in the library.
    println("macro defs accepted")
    println(classOf[Main.type].getDeclaredMethods.exists(_.getName == "s"))
  }
}
