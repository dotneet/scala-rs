// scalac: forward reference to value b defined on line 4 extends over
// definition of value a.
object Main {
  def f(): Int = { val a = b; val b = 1; a }
  def main(args: Array[String]): Unit = println(f())
}
