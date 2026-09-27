// A package object that declares a type alias and an object of the same
// name, as cats' `package object data` does for `State` and `Reader`.
// Compiled by scalac; pkgobj_term_client.scala selects through it.
package object boxes {
  type Box[A] = List[A]
  object Box {
    def one[A](a: A): Box[A] = List(a)
    def map[A, B](b: Box[A])(f: A => B): Box[B] = b.map(f)
  }
}
