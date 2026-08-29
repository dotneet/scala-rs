// Polymorphic implicit defs/vals: the search unifies a candidate's own type
// parameters with the expected type and derives its implicit arguments
// recursively. Also covers `<:<` as an ordinary implicit (`<:<.refl`) and the
// `toMap` that needs it.

trait Show[A] { def show(a: A): String }

object Show {
  implicit val showInt: Show[Int] = new Show[Int] { def show(a: Int): String = a.toString }
  implicit val showStr: Show[String] = new Show[String] { def show(a: String): String = a }

  // Derivation rules: their own `[A]` / `[A, B]` are solved from the wanted
  // type, and their implicit arguments are resolved by the same search.
  implicit def showList[A](implicit s: Show[A]): Show[List[A]] =
    new Show[List[A]] {
      def show(a: List[A]): String = a.map(s.show).mkString("[", ",", "]")
    }
  implicit def showPair[A, B](implicit sa: Show[A], sb: Show[B]): Show[(A, B)] =
    new Show[(A, B)] {
      def show(p: (A, B)): String = "(" + sa.show(p._1) + "," + sb.show(p._2) + ")"
    }
  implicit def showOpt[A](implicit s: Show[A]): Show[Option[A]] =
    new Show[Option[A]] {
      def show(o: Option[A]): String =
        if (o.isEmpty) "None" else "Some(" + s.show(o.get) + ")"
    }
}

// `Ordering[List[A]]`-shaped: a derivation whose instance is itself built from
// a derived one.
trait Ord[A] { def cmp(x: A, y: A): Int }

object Ord {
  implicit val ordInt: Ord[Int] = new Ord[Int] { def cmp(x: Int, y: Int): Int = x - y }
  implicit def ordList[A](implicit a: Ord[A]): Ord[List[A]] =
    new Ord[List[A]] {
      def cmp(x: List[A], y: List[A]): Int = {
        var xs = x
        var ys = y
        var r = 0
        while (r == 0 && xs.nonEmpty && ys.nonEmpty) {
          r = a.cmp(xs.head, ys.head)
          xs = xs.tail
          ys = ys.tail
        }
        if (r != 0) r else x.length - y.length
      }
    }
}

// A monomorphic instance is more specific than a polymorphic one (nsc
// `isAsSpecific`), so `Tag[Int]` picks `tagInt`, not `tagAny`.
trait Tag[A] { def name: String }

object Tag {
  implicit def tagAny[A]: Tag[A] = new Tag[A] { def name: String = "any" }
  implicit val tagInt: Tag[Int] = new Tag[Int] { def name: String = "int" }
}

object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)
  def compare[A](x: A, y: A)(implicit o: Ord[A]): Int = o.cmp(x, y)
  def tagOf[A](a: A)(implicit t: Tag[A]): String = t.name

  // `<:<` is derived by the general search now (`<:<.refl`), not a fallback.
  def upcast[A, B](x: A)(implicit ev: A <:< B): B = ev(x)

  def main(args: Array[String]): Unit = {
    println(render(1))
    println(render("hi"))
    println(render(List(1, 2, 3)))
    println(render(List(List(1), List(2, 3))))
    println(render((1, "x")))
    println(render(List((1, "a"), (2, "b"))))
    println(render(Option(7)))
    println(render(List(Option(1), Option(2))))

    println(compare(List(1, 2), List(1, 3)))
    println(compare(List(List(1)), List(List(1), List(2))))

    println(tagOf(1))
    println(tagOf("s"))

    val n: Any = upcast[Int, Any](42)
    println(n)

    // `toMap[K, V](implicit ev: A <:< (K, V))`: `K` and `V` appear nowhere
    // else, so the implicit search itself has to pin them down.
    val m = List((1, "a"), (2, "b")).toMap
    println(m(1) + m(2))
    val m2 = List((3, "c"), (4, "d")).iterator.toMap
    println(m2(3) + m2(4))
  }
}
