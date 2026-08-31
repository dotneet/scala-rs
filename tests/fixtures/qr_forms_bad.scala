// The quasiquote forms reification still refuses, each named.
//
// These are the ones where the parser has already normalised away something
// nsc keeps, so building anything at all would build a tree nobody wrote --
// which is worse than not compiling. `docs/macros.md` §7.7.
import scala.reflect.runtime.universe._

object QrFormsBad {
  val xs: List[Tree] = List(q"a")

  // `a :: b` is `b.::(a)` after parsing, indistinguishable from a written
  // `b.::(a)`; nsc builds neither -- it binds the left operand to a fresh
  // `val` first, to keep evaluation order.
  val rightAssoc = q"a :: b"

  // The parser supplies `()` for a missing `else`; nsc supplies an empty
  // block, and the two are different trees.
  val ifNoElse = q"if (a) b"

  // `_.get` is a lambda over a parameter the parser invented. nsc names that
  // parameter with `freshTermName`, so the trees differ in the name.
  val placeholder = q"_.get"

  // A by-name type: nsc's own parser rejects it inside `tq"..."`.
  val byName = tq"=> Int"

  // Concatenating a splice with ordinary arguments needs a static type that
  // is right on both sides; getting it wrong would reorder a call.
  val mixed = q"f(a, ..$xs)"
  val mixedStats = q"{ a; ..$xs }"

  // Definitions are reified now (`tests/fixtures/dq_defs.scala`); what is
  // still refused among them is in `tests/fixtures/dq_defs_bad.scala`. A
  // `type` definition is the one left over here.
  val typeDef = q"{ type T = Int; 1 }"

  // `cq"..."` is one clause and is written without its `case`.
  val twoCases = cq"case a => b"
}
