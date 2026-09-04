// `-Xsource-features:case-apply-copy-access` (scalac 2.13.13+): the primary
// constructor's access modifier is copied onto the synthesized `apply` and
// `copy`. Every use below is *legal* with the feature on, so the file compiles
// and runs the same either way -- what changes is the class file. Several of
// these cross a class file boundary in this compiler's lowering (`C(x)`
// becomes `C$.MODULE$.apply(x)`), which is what makes them worth running.
case class C private (x: Int) {
  def twin: C = C(x)                 // class C -> C$.apply
  def bumped: C = copy(x = x + 1)    // same class file
  def viaInner: C = new Inner().make
  class Inner {
    def make: C = C(x + 100)         // C$Inner -> C$.apply
    def bump: C = copy(x = 7)        // C$Inner -> C.copy
  }
}

object C {
  def of(x: Int): C = C(x)           // the companion's own body
}

// `protected` reaches `copy` but not `apply`: nsc's `Unapplies.applyAccess`
// only reacts to `private` / `private[p]`.
case class Prot protected (y: Int) {
  def bumped: Prot = copy(y = y + 1)
}

// The ordinary case class, unaffected by the feature.
case class Plain(w: Int)

object Main {
  def main(args: Array[String]): Unit = {
    val c = C.of(1)
    println(c)
    println(c.twin)
    println(c.bumped)
    println(c.viaInner)
    println(new c.Inner().bump)
    println(Prot(5).bumped)
    println(Plain(4).copy(w = 9))
  }
}
