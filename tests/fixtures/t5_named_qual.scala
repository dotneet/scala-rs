// Named arguments to a case class constructor reached through a *qualified*
// path (`pkg1.Bar(a = 1, b = "x")`), not a bare identifier.
//
// `reorder_named_args` resolved the parameter names to reorder against
// through `fun.sym`'s own `paramss`. For `pkg1.Bar(...)`, `fun` is a
// `Select` that resolves to the *module* `Bar` (`fun.sym.kind == Module`,
// `fun.ty == ModuleRef(...)`) -- `rewrite_receiver_apply` deliberately
// leaves a qualified companion reference unrewritten into `.apply` so
// codegen keeps emitting a direct companion-apply call (`scala.Some(1)`
// depends on this). A module symbol carries no `paramss` of its own, so
// `first_clause_ids` found nothing and reported "unimplemented syntax:
// named arguments (method parameters not resolved)" -- even though the
// exact same call written as bare `Bar(a = 1, b = "x")` (which resolves
// `fun.sym` straight to the `apply` method) already worked.
//
// The fix reads the parameter names off the module's own `apply`
// member(s) when `fun.sym` is a `Module`, the same way an overloaded
// callee already does.

package t5np {
  final case class Bar(a: Int, b: String)
}

object Main {
  def make: t5np.Bar = t5np.Bar(b = "x", a = 1)
  def main(args: Array[String]): Unit = {
    val r = make
    println(r.a)
    println(r.b)
  }
}
