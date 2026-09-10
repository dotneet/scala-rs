object Main {
  class Symbol
  def choose(x: AnyRef): String = "ref"
  def choose(x: Any)(implicit d: DummyImplicit): String = "any"
  def main(args: Array[String]): Unit = {
    val s = 'hello
    println(s.name)
    println(('hello: AnyRef) eq ('hello: AnyRef))
    println(choose('hello))
    println(s.isInstanceOf[scala.Symbol])
  }
}
