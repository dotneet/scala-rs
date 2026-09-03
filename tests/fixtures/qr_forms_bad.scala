// The quasiquote forms reification still refuses, each named.
//
// These are the ones where the parser has already normalised away something
// nsc keeps, so building anything at all would build a tree nobody wrote --
// which is worse than not compiling. `docs/macros.md` §7.7.
import scala.reflect.runtime.universe._

object QrFormsBad {
  val xs: List[Tree] = List(q"a")

  // The parser supplies `()` for a missing `else`; nsc supplies an empty
  // block, and the two are different trees.
  val ifNoElse = q"if (a) b"

  // A right-associative operator and a `_` placeholder lambda used to stand
  // here; both are reified now, out of a `freshTermName` block
  // (`tests/fixtures/fn2_fresh.scala`, `docs/macros.md` §7.10).

  // A by-name type: nsc's own parser rejects it inside `tq"..."`.
  val byName = tq"=> Int"

  // A splice among ordinary elements is reified now, as nsc's own
  // `reifyList` does it -- `List(<a>) ++ xs` (`docs/macros.md` §7.16, and
  // `tests/fixtures/sv_impl.scala` runs the result against real scalac).
  // These two are left here because they still stand for the *shape*; what is
  // still refused is a rank-2 `...$xss`, in `tests/fixtures/sv_gaps_bad.scala`.
  val mixed = q"f(a, ..$xs)"
  val mixedStats = q"{ a; ..$xs }"

  // Definitions are reified now (`tests/fixtures/dq_defs.scala`); what is
  // still refused among them is in `tests/fixtures/dq_defs_bad.scala`. A
  // `type` definition is the one left over here.
  val typeDef = q"{ type T = Int; 1 }"

  // `cq"..."` is one clause and is written without its `case`.
  val twoCases = cq"case a => b"
}
