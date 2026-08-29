// The derivation rule applies but its own implicit cannot be resolved, so the
// candidate is dropped: nsc "could not find implicit value for parameter s".
// A type parameter the search cannot pin down is never silently filled in.

trait Show[A] { def show(a: A): String }

final class Box[A](val value: A)

object Show {
  implicit def showBox[A](implicit s: Show[A]): Show[Box[A]] =
    new Show[Box[A]] {
      def show(b: Box[A]): String = "Box(" + s.show(b.value) + ")"
    }
}

object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)

  def main(args: Array[String]): Unit = {
    // No `Show[Int]` anywhere, so `showBox[Int]` cannot be built.
    println(render(new Box(1)))
  }
}
