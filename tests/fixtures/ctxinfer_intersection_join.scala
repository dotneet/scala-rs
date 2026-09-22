trait Node
trait HasName { def name: String }
trait HasSize { def size: Int }
final class LeftNode extends Node with HasName with HasSize { val name = "left"; val size = 1 }
final class RightNode extends Node with HasName with HasSize { val name = "right"; val size = 2 }
object Main {
  def selected(flag: Boolean) = if (flag) new LeftNode else new RightNode
  def optional(flag: Boolean) = (if (flag) Some((true, new LeftNode)) else Some((false, new RightNode)))
  def main(args: Array[String]): Unit = {
    println(selected(true).name)
    println(selected(false).size)
    println(optional(false).exists { case (_, node) => node.name == "right" && node.size == 2 })
  }
}
