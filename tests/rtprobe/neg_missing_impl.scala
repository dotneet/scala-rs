// scalac: class C needs to be abstract, since method f is not defined.
object Main {
  trait T { def f(x: Int): Int }
  class C extends T { def f(x: Long): Int = 1 }
  def main(args: Array[String]): Unit = println(new C().f(1L))
}
