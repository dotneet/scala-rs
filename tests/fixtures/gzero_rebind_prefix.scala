// A nested trait's singleton member must retain the prefix of the parent
// which actually contributes it.  `b.Other with a.Table` is intentionally
// ordered so an owner-only scan would choose b for both trait outer accessors.
// The two concrete roots make the wrong choice an ABI-visible ClassCastException
// as well as a type error.
trait GzeroPrefixRoot {
  type Options
  val o: Options
  trait Table { val x: o.type = o }
  trait Other
}

class GzeroPrefixA extends GzeroPrefixRoot {
  class AOptions { val marker: String = "a" }
  type Options = AOptions
  val o: AOptions = new AOptions
}

class GzeroPrefixB extends GzeroPrefixRoot {
  class BOptions { val marker: String = "b" }
  type Options = BOptions
  val o: BOptions = new BOptions
}

trait GzeroPrefixHolder {
  val a: GzeroPrefixA = new GzeroPrefixA
  val b: GzeroPrefixB = new GzeroPrefixB
  class C extends b.Other with a.Table {
    val fromA: a.o.type = x
    def marker: String = x.marker
  }
}

object GzeroPrefixMain {
  def main(args: Array[String]): Unit = {
    val h = new GzeroPrefixHolder {}
    println(new h.C().marker)
  }
}
