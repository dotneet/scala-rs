// `T.super.m` from a class nested in trait `T`: the accessor is `T`'s own
// `T$$super$m`, declared on the interface, pickled with nsc's
// `SUPERACCESSOR` so a scalac-compiled class mixing `T` in implements it,
// and resolved against that class's linearization.
package mixcgts
class B0 { def f = "B0.f" }
trait T extends B0 {
  override def f = "T.f"
  class In { def x = T.super.f }
}
trait U extends B0 { override def f = "U.f" }
