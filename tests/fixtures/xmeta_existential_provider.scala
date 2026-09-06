package xmetaexistential

trait AbstractTable[A] {
  type Elem
  def kind: String
}

class ShapedValue[A, B]

class TableQuery[E <: AbstractTable[_]](val value: E) {
  def shaped: ShapedValue[Seq[E], E#Elem] = new ShapedValue[Seq[E], E#Elem]
  def map[A](f: E => A): A = f(value)
}

class Concrete extends AbstractTable[String] {
  type Elem = String
  def kind: String = "concrete"
}

object Provider {
  val query: TableQuery[Concrete] = new TableQuery[Concrete](new Concrete)
}
