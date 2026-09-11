// A hand-written companion `apply` suppresses the synthetic case `apply` only
// at the same parameter types; and an `object`'s early definitions run before
// its traits' initializers, as a class's do.
object Main {
  case class Money(cents: Long)
  object Money { def apply(d: Double): Money = Money((d * 100).round) }
  case class Tag(name: String)
  object Tag { def apply(name: String): Tag = new Tag(name.toUpperCase) }
  case class Pt(x: Int, y: Int)
  object Pt { def apply(xy: (Int, Int)): Pt = Pt(xy._1, xy._2) }
  trait Greeter { val name: String; val msg = "Hello, " + name }
  object EarlyObj extends { val name = "object" } with Greeter
  class EarlyCls extends { val name = "class" } with Greeter
  object Plain extends Greeter { val name = "plain" }
  def main(args: Array[String]): Unit = {
    println(Money(1.235) + " " + Money(5L) + " " + List(1L, 2L).map(Money(_)))
    println(Tag("x") + " " + Pt((1, 2)) + " " + Pt(3, 4))
    println(EarlyObj.msg + " | " + new EarlyCls().msg + " | " + Plain.msg)
  }
}
