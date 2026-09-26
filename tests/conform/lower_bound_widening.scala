// `toSet[B >: A]` widens its element to the expected type. The elements of
// `List(A, B)` are `Event with Product with Serializable`; a `Set[Event]`
// where one is expected is `toSet[Event]`, as nsc infers it (gitbucket's
// `WebHook.Event.values.flatMap { ... }.toSet`).
abstract sealed class Event(val name: String)
case object A extends Event("a")
case object B extends Event("b")
object W {
  abstract sealed class Hook(val name: String)
  case object Push extends Hook("push")
  case object Fork extends Hook("fork")
  object Hook { val values = List(Push, Fork) }
}
object Main {
  val values = List(A, B)
  def direct: Set[Event] = values.toSet
  val declared: List[Event with Product with Serializable] = List(A, B)
  def fromDeclared: Set[Event] = declared.toSet
  def literal: Set[Event] = List(A, B).toSet
  def mapped: List[Event] = values.map(x => x)
  def selected(p: Map[String, String]): Set[W.Hook] =
    W.Hook.values.flatMap { t => p.get(t.name).map(_ => t) }.toSet
  def explicitArray: Array[Event] = values.toArray[Event]
  def main(args: Array[String]): Unit = {
    println(direct.map(_.name).toList.sorted)
    println((fromDeclared ++ literal).size)
    println(mapped.map(_.name))
    println(selected(Map("fork" -> "on")).map(_.name))
    val s: Set[Event] = direct + new Event("c") {}
    println(s.map(_.name).toList.sorted)
    println(explicitArray.map(_.name).mkString(","))
    // With nothing to widen to, `toSet` is `toSet[A]`: `sorted` finds an
    // `Ordering[Int]` on the result.
    println(List(3, 1, 3).toSet.toList.sorted)
    println(Vector(2, 1).toSet.toList.sorted)
  }
}
