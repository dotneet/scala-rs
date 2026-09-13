// Call sites for macro_literal_impl.scala.
import scala.language.experimental.macros

object ContextLiteral {
  def bool(): Boolean = macro ContextLiteralImpl.boolImpl
  def string(): String = macro ContextLiteralImpl.stringImpl
}

object Main {
  def main(args: Array[String]): Unit = {
    println(ContextLiteral.bool())
    println(ContextLiteral.string())
  }
}
