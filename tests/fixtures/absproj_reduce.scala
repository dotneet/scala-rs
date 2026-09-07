// A projection out of an *abstract* type (`E#ElementType`, `B#Session`) stays
// unreduced until the prefix is instantiated, and reduces then.
//
// This is the shape `slick.lifted.TableQuery[E <: AbstractTable[_]] extends
// Query[E, E#TableElementType, Seq]` is written in: with the projection
// dropped the element type came out `Any` for every table.

trait AbstractRow { type ElementType }

abstract class Rows[+E, U] {
  def label: String
  def widen: U
}

// The projection is written where `E` is still abstract; it must not be read
// as `AbstractRow`'s own deferred `ElementType`, and it must not be `Any`.
abstract class RowSet[E <: AbstractRow] extends Rows[E, E#ElementType]

class Account extends AbstractRow { type ElementType = (String, Int) }

class Accounts extends RowSet[Account] {
  def label = "accounts"
  def widen: (String, Int) = ("ada", 36)
}

// A subclass that keeps `E` abstract keeps the projection: its own base type
// is `Rows[E, E#ElementType]`, not `Rows[E, Any]`.
abstract class NamedRowSet[E <: AbstractRow] extends RowSet[E] {
  def label = "named"
}

class NamedAccounts extends NamedRowSet[Account] {
  def widen: (String, Int) = ("grace", 45)
}

// The same question through a type *member* prefix rather than a type
// parameter: `slick.basic.BasicProfile`'s `type Session = Backend#Session`.
trait BaseBackend { type Session }
class SessionDef { def label: String = "session" }
trait ConcreteBackend extends BaseBackend { type Session = SessionDef }

abstract class Runner[B <: BaseBackend] {
  def use(s: B#Session): String
}

class ConcreteRunner extends Runner[ConcreteBackend] {
  def use(s: SessionDef): String = s.label
}

object Main {
  def main(args: Array[String]): Unit = {
    val accounts = new Accounts
    val q: Rows[Account, (String, Int)] = accounts
    println(q.label)
    println(q.widen)

    val named: Rows[Account, (String, Int)] = new NamedAccounts
    println(named.label)
    println(named.widen._1)

    val r: Runner[ConcreteBackend] = new ConcreteRunner
    println(r.use(new SessionDef))
  }
}
