package p1.p2

object B { def f: Int = 10 }
class C2 { def g: Int = 20 }

// A `case class` gets a synthetic companion before its written `object` is
// named, so both answer to `QP`; `import p1.p2.QP.op` must find the written
// one.
final case class QP(a: Int)
object QP { def op(n: String): Int = n.length }
