// Type classes: context bounds, implicitly, derived instances via implicit
// defs, instances for tuples and Options, and syntax via implicit classes.
object Main {
  trait Monoid[A] { def empty: A; def combine(x: A, y: A): A }
  object Monoid {
    def apply[A](implicit m: Monoid[A]): Monoid[A] = m
    implicit val intM: Monoid[Int] = new Monoid[Int] { def empty = 0; def combine(x: Int, y: Int) = x + y }
    implicit val strM: Monoid[String] = new Monoid[String] { def empty = ""; def combine(x: String, y: String) = x + y }
    implicit def optM[A: Monoid]: Monoid[Option[A]] = new Monoid[Option[A]] {
      def empty = None
      def combine(x: Option[A], y: Option[A]) = (x, y) match { case (Some(a), Some(b)) => Some(Monoid[A].combine(a, b)); case (a, None) => a; case (None, b) => b }
    }
    implicit def pairM[A: Monoid, B: Monoid]: Monoid[(A, B)] = new Monoid[(A, B)] {
      def empty = (Monoid[A].empty, Monoid[B].empty)
      def combine(x: (A, B), y: (A, B)) = (Monoid[A].combine(x._1, y._1), Monoid[B].combine(x._2, y._2))
    }
    implicit def mapM[K, V: Monoid]: Monoid[Map[K, V]] = new Monoid[Map[K, V]] {
      def empty = Map.empty
      def combine(x: Map[K, V], y: Map[K, V]) = y.foldLeft(x) { case (acc, (k, v)) => acc.updated(k, acc.get(k).fold(v)(Monoid[V].combine(_, v))) }
    }
  }
  implicit class MonoidOps[A](private val a: A) extends AnyVal { def |+|(b: A)(implicit m: Monoid[A]): A = m.combine(a, b) }
  def combineAll[A: Monoid](xs: List[A]): A = xs.foldLeft(Monoid[A].empty)(_ |+| _)

  trait Show[A] { def show(a: A): String }
  implicit val showInt: Show[Int] = i => s"Int($i)"
  implicit def showList[A](implicit s: Show[A]): Show[List[A]] = l => l.map(s.show).mkString("L[", ",", "]")
  def show[A: Show](a: A): String = implicitly[Show[A]].show(a)

  def main(args: Array[String]): Unit = {
    println(combineAll(List(1, 2, 3)) + " " + combineAll(List("a", "b")) + " " + combineAll(List(Option(1), None, Option(5))))
    println(combineAll(List((1, "x"), (2, "y"))) + " " + combineAll(List.empty[(Int, String)]))
    println(combineAll(List(Map("a" -> 1), Map("a" -> 2, "b" -> 3))).toList.sorted)
    println((3 |+| 4) + " " + ("p" |+| "q") + " " + (Option(2) |+| Option(3)))
    println(show(5) + " " + show(List(1, 2)) + " " + show(List(List(3))))
    println(Monoid[Option[Int]].empty + " " + Monoid[(Int, String)].empty)
  }
}
