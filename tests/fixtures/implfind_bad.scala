// `implfind.scala` が緩めたアクセス規則の裏側。どちらも nsc が拒否する。
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
  // コンパニオンでもサブクラスでもない: protected は見えない。
  class Stranger {
    def a: Int = implfindbad.Prot.hidden
  }

  // `private[implfindbad]` はパッケージの外からは見えない。
  class Outsider {
    def b: Int = implfindbad.Outer.Inner.v
  }
}
