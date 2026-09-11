// scalac: `method f needs `override' modifier`.
object Main {
  class A { def f: Int = 1 }
  class B extends A { def f: Int = 2 }
  def main(args: Array[String]): Unit = println(new B().f)
}
