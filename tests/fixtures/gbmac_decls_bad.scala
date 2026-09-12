// Classes the engine's mirror cannot describe in nsc's shape, asked about by
// the same macro as `gbmac_decls_use.scala`. Real scalac 2.13.16 compiles and
// runs this file; scala-rs refuses each call site, naming the class member it
// could not translate (`crates/typer/src/expand_mirror.rs`). A declaration
// list with that member left out would be a different class, and a macro
// that walks `decls` would act on it.
package gbmac
package bad

// A nested class: nsc lists a class symbol among the declarations, and the
// mirror has none to offer.
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
// package `bad`) and no PRIVATE flag, which the mirror has no way to say.
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
