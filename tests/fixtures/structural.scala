class C {
  def foo: Int = 42
}
object Main {
  def use(x: { def foo: Int }): Int = x.foo
  def main(args: Array[String]): Unit = {
    println(use(new C()))
  }
}
