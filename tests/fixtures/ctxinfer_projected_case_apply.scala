trait Kind { type Data }
class TextKind extends Kind { type Data = String }
class DerivedText extends TextKind
class IntKind extends Kind { type Data = Int }
trait Show[A] { def render(value: A): String }
object Show {
  implicit object TextShow extends Show[String] { def render(value: String): String = value }
  implicit object IntShow extends Show[Int] { def render(value: Int): String = value.toString }
}
case class Linked[K <: Kind](kind: K)(val value: K#Data)(implicit show: Show[K#Data]) {
  def render: String = show.render(value)
}
case class Bounded[K <: Kind, V <: K#Data](kind: K, value: V)
object Main {
  def main(args: Array[String]): Unit = {
    println(Linked(new DerivedText)("text").render)
    println(Linked[IntKind](new IntKind)(7).render)
    println(Bounded(new TextKind, "bound").value)
  }
}
