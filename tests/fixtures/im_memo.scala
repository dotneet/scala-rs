// Implicit search is memoized by `(wanted type, undetermined call-site
// parameters, depth)` -- `ImplicitMemo` in `crates/typer/src/implicits.rs`.
// This exercises the three properties that memo has to preserve.
//
//  * The same wanted type is reached more than once inside *one* derivation,
//    at the same depth (`Show[(Int, Int)]`) and at different depths
//    (`Show[Int]` under a list, under an option, and under a pair), and every
//    reader has to get the witness the first search picked.
//  * A candidate that fails at depth 8 and one that succeeds at depth 2 are
//    two different answers to the same question, so the memo cannot drop the
//    depth from its key. `deep` nests eight constructors.
//  * The memo cannot outlive one search: `shadowed` binds a nearer
//    `Show[Int]`, and every derivation inside it has to see that one instead
//    of the object-level witness, even though the object-level answer was
//    memoized moments earlier.
object Main {
  trait Show[A] { def show(a: A): String }

  implicit val showInt: Show[Int] = new Show[Int] { def show(a: Int) = "i" + a }
  implicit val showStr: Show[String] = new Show[String] { def show(a: String) = "s" + a }

  implicit def showList[A](implicit s: Show[A]): Show[List[A]] =
    new Show[List[A]] {
      def show(a: List[A]) = a.map(s.show).mkString("[", ",", "]")
    }

  implicit def showOpt[A](implicit s: Show[A]): Show[Option[A]] =
    new Show[Option[A]] {
      def show(a: Option[A]) = a.map(s.show).getOrElse("-")
    }

  implicit def showPair[A, B](implicit a: Show[A], b: Show[B]): Show[(A, B)] =
    new Show[(A, B)] {
      def show(p: (A, B)) = "(" + a.show(p._1) + "," + b.show(p._2) + ")"
    }

  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)

  // The nearer binding wins, which it can only do if the memo does not survive
  // the search that filled it.
  def shadowed: String = {
    implicit val showInt: Show[Int] = new Show[Int] { def show(a: Int) = "I" + a }
    render(List(1, 2)) + render((3, 4))
  }

  // Eight nested constructors: the innermost `Show[Int]` is asked for at a
  // depth the outer ones never reach.
  def deep: String =
    render(List(Option(List(Option(List(Option((1, "z"))))))))

  def main(args: Array[String]): Unit = {
    println(render(List(1, 2)))
    println(render((7, 7)))
    println(render(List(Option((1, "a")), None)))
    println(shadowed)
    println(deep)
  }
}
