// A small query AST in the shape slick's `ast` package uses: a sealed
// hierarchy, matched with guards, nested patterns, `@` bindings and type
// patterns mixed in one match.
object Main {
  sealed trait Node { def children: List[Node] = Nil }
  case class Lit(v: Any) extends Node
  case class Col(table: String, name: String) extends Node
  case class Bin(op: String, l: Node, r: Node) extends Node {
    override def children = List(l, r)
  }
  case class Not(n: Node) extends Node { override def children = List(n) }
  case class Select(from: String, where: Option[Node], cols: List[Node]) extends Node {
    override def children = where.toList ::: cols
  }

  def show(n: Node): String = n match {
    case Lit(s: String) => "'" + s + "'"
    case Lit(null) => "NULL"
    case Lit(v) => v.toString
    case Col(t, c) => t + "." + c
    case Bin("=", l, Lit(null)) => show(l) + " IS NULL"
    case b @ Bin(op, l, r) if b.op == "AND" || op == "OR" =>
      "(" + show(l) + " " + op + " " + show(r) + ")"
    case Bin(op, l, r) => show(l) + " " + op + " " + show(r)
    case Not(Not(inner)) => show(inner)
    case Not(n2) => "NOT " + show(n2)
    case Select(f, w, cs) =>
      "SELECT " + cs.map(show).mkString(", ") + " FROM " + f +
        w.map(x => " WHERE " + show(x)).getOrElse("")
  }

  def simplify(n: Node): Node = n match {
    case Not(Not(x)) => simplify(x)
    case Bin("AND", Lit(true), r) => simplify(r)
    case Bin("AND", l, Lit(true)) => simplify(l)
    case Bin(op, l, r) => Bin(op, simplify(l), simplify(r))
    case Not(x) => Not(simplify(x))
    case Select(f, w, cs) => Select(f, w.map(simplify), cs.map(simplify))
    case other => other
  }

  def count(n: Node): Int = 1 + n.children.map(count).sum

  def main(args: Array[String]): Unit = {
    val q = Select(
      "users",
      Some(Bin("AND", Lit(true), Bin("OR", Bin("=", Col("users", "name"), Lit("bo")),
        Not(Not(Bin("=", Col("users", "age"), Lit(null))))))),
      List(Col("users", "id"), Lit(1), Lit(2.5))
    )
    println(show(q))
    println(show(simplify(q)))
    println(count(q))
    println(count(simplify(q)))
    val ns: List[Node] = List(Lit(3), Col("t", "c"), Not(Lit(false)), Bin("+", Lit(1), Lit(2)))
    ns.foreach { n =>
      val tag = n match {
        case _: Lit => "lit"
        case c: Col => "col:" + c.name
        case Not(_) => "not"
        case _ => "other"
      }
      println(tag + " -> " + show(n))
    }
  }
}
