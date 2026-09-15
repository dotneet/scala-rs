// The singleton inherited from a.Table is not b.o.type.  Keep this negative
// sibling of gzero_rebind_prefix.scala in the differential parity controls.
trait GzeroPrefixBadRoot {
  type Options
  val o: Options
  trait Table { val x: o.type = o }
  trait Other
}

class GzeroPrefixBadA extends GzeroPrefixBadRoot {
  class AOptions { val marker: String = "a" }
  type Options = AOptions
  val o: AOptions = new AOptions
}

class GzeroPrefixBadB extends GzeroPrefixBadRoot {
  class BOptions { val marker: String = "b" }
  type Options = BOptions
  val o: BOptions = new BOptions
}

trait GzeroPrefixBadHolder {
  val a: GzeroPrefixBadA = new GzeroPrefixBadA
  val b: GzeroPrefixBadB = new GzeroPrefixBadB
  class C extends b.Other with a.Table {
    val fromA: a.o.type = x
    val wrong: b.o.type = x
  }
}
