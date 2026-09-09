trait Node { def root: Node }
trait Root { self: Node => val root = this }
final class RootNode extends Node with Root
object Main { def main(args: Array[String]): Unit = { val n = new RootNode; println(n.root eq n) } }
