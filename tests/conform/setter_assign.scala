class C {
  private var raw = 0
  def foo: Int = raw
  def foo_=(v: Int): Unit = { raw = v * 2 }
  var plain = 1
}
trait T { var tv: Int = 0 }
class D extends T
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C
    c.foo = 4
    println(c.foo)
    c.plain = 7
    println(c.plain)
    val d = new D
    d.tv = 9
    println(d.tv)
    var local = 1
    local = 2
    println(local)
  }
}
