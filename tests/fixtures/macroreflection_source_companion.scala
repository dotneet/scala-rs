package sourcecompanion {
  case class Entry(value: Int)
  object Entry {
    def apply(value: Int): Entry = new Entry(value)
  }
}

object Main extends App {
  val generic = shapeless.Generic[sourcecompanion.Entry]
  val value = sourcecompanion.Entry(42)
  println(generic.from(generic.to(value)))
}
