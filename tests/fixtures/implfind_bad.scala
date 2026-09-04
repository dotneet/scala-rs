// The far side of the access rules `implfind.scala` relaxed. nsc rejects both.
package implfindbad {

  trait Prot {
    def viaTrait: Int = Prot.hidden
  }
  object Prot {
    protected val hidden: Int = 7
  }

  object Outer {
    private[implfindbad] object Inner { val v: Int = 13 }
  }
}

package other {
  // Neither the companion nor a subclass: `protected` is not visible.
  class Stranger {
    def a: Int = implfindbad.Prot.hidden
  }

  // `private[implfindbad]` is not visible from outside the package.
  class Outsider {
    def b: Int = implfindbad.Outer.Inner.v
  }
}
