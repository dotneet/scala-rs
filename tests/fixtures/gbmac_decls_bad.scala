// Formerly unsupported reflection shapes, compared with scalac.
package gbmac
package bad

// A nested class appears among the reflected declarations.
class Outer {
  class Inner
  def f: Int = 1
}

// A type member.
class WithType {
  type Elem = Int
  def g: Elem = 2
}

// A case class with a second parameter list, which nsc does not make case
// accessors of.
case class TwoLists(a: Int)(b: String)

// A qualified access boundary: nsc's symbol carries `privateWithin` (the
// package `bad`) and no PRIVATE flag.
class Guarded {
  private[bad] def h: Int = 3
}

object Main {
  def main(args: Array[String]): Unit = {
    print(Decls.show[Outer])
    print(Decls.show[WithType])
    print(Decls.show[TwoLists])
    print(Decls.show[Guarded])
  }
}
