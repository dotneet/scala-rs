// Companion objects: apply/unapply written by hand, private constructors
// reached from the companion, companion-private members, update, and
// objects used as functions.
object Main {
  class Temp private (val kelvin: Double) {
    private def secret = kelvin * 2
    override def toString = f"Temp($kelvin%.2fK)"
  }
  object Temp {
    def apply(c: Double): Temp = new Temp(c + 273.15)
    def unapply(t: Temp): Option[Double] = Some(t.kelvin - 273.15)
    def peek(t: Temp): Double = t.secret
    val zero: Temp = apply(0)
  }
  class Grid(val w: Int, val h: Int) {
    private val cells = Array.fill(w * h)(0)
    def apply(x: Int, y: Int): Int = cells(y * w + x)
    def update(x: Int, y: Int, v: Int): Unit = cells(y * w + x) = v
  }
  object Twice extends (Int => Int) { def apply(x: Int): Int = x * 2 }
  object Counter { private var n = 0; def apply(): Int = { n += 1; n } }
  case class Money(cents: Long)
  object Money { def apply(d: Double): Money = Money((d * 100).round); implicit val ord: Ordering[Money] = Ordering.by(_.cents) }
  trait Shape
  object Shape { def apply(kind: String): Shape = kind match { case "c" => new Circle; case _ => new Square }; class Circle extends Shape { override def toString = "circle" }; class Square extends Shape { override def toString = "square" } }

  def main(args: Array[String]): Unit = {
    val t = Temp(25)
    println(t); println(Temp.peek(t) > 500); println(Temp.zero)
    t match { case Temp(c) => println(f"celsius $c%.1f") }
    val g = new Grid(3, 2)
    g(1, 1) = 5; g(2, 0) += 7
    println(g(1, 1) + " " + g(2, 0) + " " + g(0, 0))
    println(Twice(21) + " " + List(1, 2).map(Twice) + " " + (Twice andThen Twice)(1))
    println(Counter() + Counter() + Counter())
    println(Money(1.235) + " " + Money(5L) + " " + List(Money(3L), Money(1L)).sorted + " " + List(Money(2.0), Money(1.0)).max)
    println(Shape("c") + " " + Shape("s"))
    val f: Int => Int = Twice
    println(f(4))
    val ctor: (Long) => Money = Money(_: Long)
    println(ctor(9L))
    println(List(1L, 2L).map(Money(_)))
  }
}
