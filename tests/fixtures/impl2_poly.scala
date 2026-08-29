// Polymorphic implicit derivation with no library types, so it runs in both
// the private-runtime and the scala-library mode.

trait Show[A] { def show(a: A): String }

final class Box[A](val value: A)
final class Pair[A, B](val first: A, val second: B)

object Show {
  implicit val showInt: Show[Int] = new Show[Int] { def show(a: Int): String = a.toString }
  implicit val showStr: Show[String] = new Show[String] { def show(a: String): String = a }

  // The search solves `A` from the wanted type and then resolves `s` itself,
  // recursively: `Show[Box[Box[Int]]]` needs `showBox[Box[Int]](showBox[Int](showInt))`.
  implicit def showBox[A](implicit s: Show[A]): Show[Box[A]] =
    new Show[Box[A]] {
      def show(b: Box[A]): String = "Box(" + s.show(b.value) + ")"
    }

  implicit def showPair[A, B](implicit sa: Show[A], sb: Show[B]): Show[Pair[A, B]] =
    new Show[Pair[A, B]] {
      def show(p: Pair[A, B]): String = "<" + sa.show(p.first) + "," + sb.show(p.second) + ">"
    }
}

// nsc `isAsSpecific`: the monomorphic instance beats the polymorphic one.
trait Tag[A] { def name: String }

object Tag {
  implicit def tagAny[A]: Tag[A] = new Tag[A] { def name: String = "any" }
  implicit val tagInt: Tag[Int] = new Tag[Int] { def name: String = "int" }
}

object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)
  def tagOf[A](a: A)(implicit t: Tag[A]): String = t.name

  def main(args: Array[String]): Unit = {
    println(render(1))
    println(render(new Box(2)))
    println(render(new Box(new Box(3))))
    println(render(new Pair(4, "four")))
    println(render(new Box(new Pair(5, "five"))))
    println(render(new Pair(new Box(6), new Box("six"))))
    println(tagOf(7))
    println(tagOf("seven"))
  }
}
