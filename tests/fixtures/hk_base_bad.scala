// Reading an argument's base type must not make it fit *anything*: the base
// type's own type arguments still have to agree.
object Main {
  trait D[A]
  object OD extends D[Int]
  def need(d: D[String]): Int = 0
  def two[A](x: D[A], y: A): Int = 0
  def main(args: Array[String]): Unit = {
    println(need(OD))
    println(two(OD, "s"))
  }
}
