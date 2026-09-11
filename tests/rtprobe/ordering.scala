// Ordering: derived orderings (by, reverse, orElse, tuples, Option),
// custom Ordered classes, compare semantics for Double (NaN, -0.0), and
// min/max/sorted using an implicit ordering in scope.
object Main {
  case class Ver(major: Int, minor: Int) extends Ordered[Ver] {
    def compare(o: Ver): Int = if (major != o.major) major - o.major else minor - o.minor
  }
  case class Item(name: String, price: Double)
  def main(args: Array[String]): Unit = {
    val vs = List(Ver(1, 2), Ver(0, 9), Ver(1, 0))
    println(vs.sortWith(_ < _) + " " + (Ver(1, 0) < Ver(1, 1)) + " " + (Ver(2, 0) >= Ver(1, 9)) + " " + Ver(1, 1).compareTo(Ver(1, 1)))
    val items = List(Item("b", 2.5), Item("a", 2.5), Item("c", 1.0))
    val byPrice = Ordering.by[Item, Double](_.price)
    println(items.sorted(byPrice).map(_.name) + " " + items.sorted(byPrice.reverse).map(_.name))
    println(items.sorted(byPrice.orElseBy(_.name)).map(_.name))
    println(items.sorted(Ordering.by((i: Item) => (i.price, i.name))).map(_.name))
    println(List(Some(3), None, Some(1)).sorted + " " + List((2, "a"), (1, "b"), (2, "0")).sorted)
    println(List(3.0, Double.NaN, -0.0, 0.0, -1.0).sorted + " " + List(1.0, Double.NaN).max + " " + List(1.0, Double.NaN).min)
    println(Ordering[Double].compare(0.0, -0.0) + " " + Ordering.Double.TotalOrdering.compare(Double.NaN, 1.0) + " " + java.lang.Double.compare(0.0, -0.0))
    println(math.max(0.0, -0.0) + " " + math.min(0.0, -0.0) + " " + (Double.NaN max 1.0))
    locally {
      implicit val lenOrd: Ordering[String] = Ordering.by(_.length)
      println(List("ccc", "a", "bb").sorted + " " + List("ccc", "a", "bb").max + " " + List("xx", "y").min)
    }
    println(Ordering[Int].lt(1, 2) + " " + Ordering[Int].max(3, 4) + " " + Ordering[String].equiv("ab", "cd") + " " + Ordering.Int.reverse.compare(1, 2))
    val ord = Ordering[(Int, String)]
    println(ord.compare((1, "b"), (1, "a")) > 0)
    println(List('c', 'a', 'b').sorted + " " + List(3L, 1L).sorted + " " + List(true, false).sorted)
    import scala.math.Ordered.orderingToOrdered
    println((1, "a") < ((1, "b")))
    println(List(Ver(0, 1), Ver(0, 0)).sortWith(_ > _))
    println(List("b", "a").sorted(Ordering.String) + " " + List("b", "A").sortBy(_.toLowerCase)(Ordering.String))
    val cmp = Ordering.fromLessThan[Int](_ > _)
    println(List(1, 3, 2).sorted(cmp))
    println(Seq(1, 2, 3).sorted(Ordering.Int.on[Int](x => -x)))
  }
}
