// The cases real scalac 2.13.16 rejects, and that a reduction firing too
// early would silently accept.
//
// The load-bearing one is the second: `E#ElementType` where `E` is still
// abstract is *not* `AbstractRow`'s `ElementType` and is *not* `Any`. A
// reduction that read the prefix's bound would make every `RowSet[E]` a
// `Rows[E, Any]`, and with it every unrelated element type.

trait AbstractRow { type ElementType }

abstract class Rows[+E, U] {
  def label: String
  def widen: U
}

abstract class RowSet[E <: AbstractRow] extends Rows[E, E#ElementType]

class Account extends AbstractRow { type ElementType = (String, Int) }

class Accounts extends RowSet[Account] {
  def label = "accounts"
  def widen: (String, Int) = ("ada", 36)
}

object Bad {
  // The reduction really did fire: the element type is `(String, Int)`, so
  // this is not a `Rows[Account, String]`.
  val wrongElem: Rows[Account, String] = new Accounts

  // Not reduced through the *bound*: with `E` abstract, `E#ElementType` is
  // neither `Any` nor `AbstractRow`'s own declaration.
  def widened[E <: AbstractRow](rs: RowSet[E]): Rows[E, Any] = rs
}

// Still accepted, and real scalac rejects it -- a *known limit*, not a case
// this fixture pins:
//
//   def crossed[E <: AbstractRow, F <: AbstractRow](rs: RowSet[E]): Rows[E, F#ElementType] = rs
//
// `RowSet[E]` at an `E` that settles nothing gives back the bare declaration
// rather than a projection through the new prefix (rebuilding one needs
// `&mut SymbolTable` inside `subst_tparams`), and the bare declaration
// conforms to every projection of itself. That is exactly the answer this
// compiler gave before projections existed, so nothing regressed -- but it is
// the one shape where the reduction is less precise than nsc. See
// `docs/gitbucket.md`, "A projection out of an abstract type".
