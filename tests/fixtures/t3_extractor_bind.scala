// `x @ Extractor(...)` narrows `x` to the extractor's own declared receiver
// type, not the scrutinee's static type -- exactly as `case x: T` does. This
// is slick's `LiteralNode` / `Node` pattern from `SQLServerProfile.scala` and
// `JdbcStatementBuilderComponent.scala`, reduced.
trait Node {
  def tag: String
}

class LiteralNode(val value: Any, val volatileHint: Boolean = false) extends Node {
  def tag: String = "literal"
}

class OtherNode extends Node {
  def tag: String = "other"
}

object LiteralNode {
  def unapply(n: LiteralNode): Option[Any] = Some(n.value)
}

// A generic extractor whose receiver type shares the scrutinee's own type
// parameter: unifying the extractor's type parameter against the scrutinee
// must still produce a receiver type, not just fall back to raw params.
class Box[T](val get: T)
object NonEmptyBox {
  def unapply[T](b: Box[T]): Option[T] = Some(b.get)
}

object Main {
  def describe(n: Node): String = n match {
    case c @ LiteralNode(v) if c.volatileHint => s"volatile literal $v"
    case LiteralNode(v) => s"literal $v"
    case o => o.tag
  }

  def unwrap[T](b: Box[T]): T = b match {
    case bx @ NonEmptyBox(v) => bx.get
  }

  def main(args: Array[String]): Unit = {
    println(describe(new LiteralNode("a", true)))
    println(describe(new LiteralNode("b", false)))
    println(describe(new OtherNode))
    println(unwrap(new Box(42)))
  }
}
