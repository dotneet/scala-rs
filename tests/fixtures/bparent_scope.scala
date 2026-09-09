package bparentscope.base {
  trait Bits
  object Bits { class Proxy(val coll: Bits) { protected val elems: Int = 7 } }
}
package bparentscope.local {
  class Bits extends bparentscope.base.Bits
  object Bits {
    def fromMask(value: Int): Int = value + 10
    class Proxy(coll: Bits) extends bparentscope.base.Bits.Proxy(coll) {
      def result: Int = Bits.fromMask(elems)
    }
  }
}
object Main {
  def main(args: Array[String]): Unit =
    println(new bparentscope.local.Bits.Proxy(new bparentscope.local.Bits).result)
}
