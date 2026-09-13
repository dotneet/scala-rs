// The other direction for `lf_seen.scala`: not substituting twice must not turn
// into not substituting at all.
trait Elem[A, C] {
  def readOne(c: Cell[Elem[A, C]]): Elem[A, C] = c.v
  // `c.v` really is an `Elem[A, C]` and nothing else: a `C` is not one.
  def wrongResult(c: Cell[Elem[A, C]]): C = c.v
  // ...and the argument still has to be the cell the parameter names.
  def wrongArg(c: Cell[A]): Elem[A, C] = readOne(c)
}

class Cell[T](val v: T) extends Elem[T, Cell[T]]

class UsesElem extends Elem[Int, String] {
  // A member read through a base class is still read at *that* class's
  // arguments: `readOne` wants a `Cell[Elem[Int, String]]`.
  def bad: Elem[Int, String] = readOne(new Cell[Int](1))
}
