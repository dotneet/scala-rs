// scalac: cannot override final member.
object Main {
  class A { final def f: Int = 1 }
  class B extends A { override def f: Int = 2 }
  def main(args: Array[String]): Unit = println(new B().f)
}
