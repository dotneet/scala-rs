import demo.ast.Node
import demo.util.Box

object Main {
  def main(args: Array[String]): Unit = {
    val n = Node("a", Box.make(3))
    println(n.name)
    println(n.box.doubled)
    println(demo.util.Box.make(5).x)
    println(n)
  }
}
