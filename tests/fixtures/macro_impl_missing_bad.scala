// The implementation reference has to resolve to something.
object Sugar {
  def f(): Int = macro Macros.noSuchImpl
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
