object Main {
  def foreignKey[T](target: T)(pick: T => Int): Int = pick(target)

  def check(): Int = {
    class A {
      def value = foreignKey(bs)(b => b.f1)
    }
    lazy val bs = new B
    class B {
      val f1: Int = 42
    }
    new A().value
  }

  def main(args: Array[String]): Unit = println(check())
}
