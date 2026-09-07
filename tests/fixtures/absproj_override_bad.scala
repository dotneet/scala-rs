// The reduced projection is what an implementation has to meet.
//
// `RowSet[Account]` inherits `def widen: E#ElementType` with `E := Account`,
// which is `(String, Int)`. Defining `widen: String` leaves the inherited
// member undefined -- it does not override it -- and real scalac 2.13.16
// says so. Kept in its own file because scalac's override check runs after
// the typer and never reports when the typer already has.

trait AbstractRow { type ElementType }

abstract class Rows[+E, U] {
  def widen: U
}

abstract class RowSet[E <: AbstractRow] extends Rows[E, E#ElementType]

class Account extends AbstractRow { type ElementType = (String, Int) }

class WrongWiden extends RowSet[Account] {
  def widen: String = "ada"
}
