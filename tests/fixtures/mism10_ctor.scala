// Two things a class *header* needs that the signature pass could not give it.
// No library type is involved, so this runs on the private runtime too.
//
//  * A parent's constructor arguments are ordinary expressions. The signature
//    pass types them before every unit's members have their types, so slick's
//    `extends Ordered(Vector((column.toNode, ord)))` saw a `Rep[T]` whose
//    `toNode` was not yet a member and reported
//    `found: Vector[(T1, Ordering)]`. The body pass types the very same tree
//    again and gets it right, so the signature pass's complaint about a parent
//    constructor's arguments is dropped -- exactly as the header pass's is.
//
//  * A primary constructor declares no type parameters of its own: `A` belongs
//    to the *class*, and a defaulted parameter's body has no
//    `name$default$n` getter to be typed in, so it is typed at the call site
//    where nothing binds that name. `class C[A](l: Chain[A] = Chain.empty[A])`
//    reported `found: Chain[A]  required: Chain[A]`, the found one being an
//    unresolved name rather than the class's `A`.

class Chain[A](val head: A, val rest: Chain[A]) {
  def size: Int = if (rest == null) 1 else 1 + rest.size
  def show: String = if (rest == null) head.toString else head.toString + "," + rest.show
}

object Chain {
  def empty[A]: Chain[A] = null
  def of[A](a: A): Chain[A] = new Chain[A](a, null)
  def size[A](c: Chain[A]): Int = if (c == null) 0 else c.size
  def show[A](c: Chain[A]): String = if (c == null) "-" else c.show
}

class Base(val cols: Chain[(Node, Ord)])

case class ColOrdered[T](column: Rep[T], ord: Ord) extends Base(Chain.of((column.toNode, ord)))

class Node(val s: String)
class Ord(val name: String)
trait Rep[T] { def toNode: Node }

class Box[A](val one: Chain[A] = Chain.empty[A], val two: Int = 7) {
  def total: Int = Chain.size(one) + two
}

class HkBox[F[_]](val cell: Cell[F] = Cell.empty[F]) {
  def tag: String = cell.tag
}

trait Cell[F[_]] { def tag: String }
object Cell {
  def empty[F[_]]: Cell[F] = new Cell[F] { def tag = "empty" }
}

object Main {
  def main(args: Array[String]): Unit = {
    val r = new Rep[Int] { def toNode = new Node("n") }
    val c = ColOrdered(r, new Ord("asc"))
    val p = c.cols.head
    println(p._1.s + ":" + p._2.name)

    println(new Box[String]().total)
    println(new Box[String](Chain.of("a")).total)
    println(new Box[String](Chain.of("a"), 1).total)
    println(Chain.show(new Box[String](Chain.of("z")).one))
    println(new HkBox[Chain]().tag)
    println(new HkBox[Chain](new Cell[Chain] { def tag = "given" }).tag)
  }
}
