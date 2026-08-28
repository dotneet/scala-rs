// Explicit type arguments are checked against the bounds too.
trait Named {
  def name: String
}

object Main {
  def f[A <: Named](x: A): Int = 1

  def main(args: Array[String]): Unit = {
    println(f[Int](42))
  }
}
