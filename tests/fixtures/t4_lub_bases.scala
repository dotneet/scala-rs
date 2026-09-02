// The join of two branches has to be the *least* of their common upper
// bounds, not the first one found walking one branch's ancestry.
//
// `Nn.type` and `Sm[String]` share `Opt[String]` and `Marker`. Walking
// `Nn.type`'s own chain reaches `Marker` first -- `Sm[String] <: Opt[Nothing]`
// is false (`String` is not `<: Nothing`), and `Marker` is next -- so the old
// first-match join answered `Marker` and `.get` was not a member of it.
//
// This is `Option` in miniature: `scala/Option`'s classfile declares
// `implements scala.Product`, so in a run where anything had made that parent
// visible, `lub(None, Some(x))` answered `Product`, and slick's
// `PositionedResult`'s `nextBlobOption() getOrElse (…)` -- whose receiver is
// `if (rs.wasNull) None else Some(r)` -- was `value getOrElse is not a member
// of Product`. The same shape written out here needs no library state, so it
// fails on plain `main` too.

object Main {
  trait Marker
  sealed abstract class Opt[+A] extends Marker {
    def get: A
    def orElse[B >: A](d: B): B = if (isEmpty) d else get
    def isEmpty: Boolean
  }
  case object Nn extends Opt[Nothing] {
    def get = throw new RuntimeException("empty")
    def isEmpty = true
  }
  final case class Sm[+A](v: A) extends Opt[A] {
    def get = v
    def isEmpty = false
  }

  // No declared result type: the branches' join is the whole answer.
  def pick(c: Boolean, s: String) = { val r = if (c) Nn else Sm(s); r }
  def pickInt(c: Boolean, n: Int) = { val r = if (c) Nn else Sm(n); r }
  // The other order, too.
  def pickFlipped(c: Boolean, s: String) = if (c) Sm(s) else Nn

  def main(args: Array[String]): Unit = {
    println(pick(false, "y").get)
    println(pick(true, "y").orElse("z"))
    println(pickInt(false, 3).get + 1)
    println(pickFlipped(true, "f").get)
  }
}
