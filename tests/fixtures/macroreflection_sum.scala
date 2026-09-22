object Main {
  sealed trait Event
  final case class Added(value: Int) extends Event
  case object Cleared extends Event
  val generic = shapeless.Generic[Event]
  def main(args: Array[String]): Unit = {
    println(generic.from(generic.to(Added(7))))
    println(generic.from(generic.to(Cleared)))
  }
}
