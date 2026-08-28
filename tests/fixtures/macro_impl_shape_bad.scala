// The implementation must be a method of an object whose first parameter is a
// macro `Context`. `notAnImpl` is an ordinary method.
import scala.reflect.macros.blackbox.Context

object Macros {
  def notAnImpl(x: Int): Int = x
}

object Sugar {
  def f(x: Int): Int = macro Macros.notAnImpl
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
