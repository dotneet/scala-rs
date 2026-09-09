trait Parent { def name: Int = 1 }
class Child extends Parent {
  def register(body: => Unit): Unit = body
  register { val name = "local"; println(name) }
}
object Main { def main(args: Array[String]): Unit = { val c = new Child; println(c.name) } }
