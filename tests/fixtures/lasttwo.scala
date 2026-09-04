// agent/lasttwo: the four defects between `tests/slick_run.sh` and twelve
// completed programs (it was `ok=10 diff=1 fail=1`).
//
// 1. A primary constructor's default arguments had no getters at all, so a
//    *separately compiled* caller could not link: nsc puts
//    `$lessinit$greater$default$n` -- and, for a case class, `apply$default$n`
//    -- on the companion module. slick's `case class Length(length: Int,
//    varying: Boolean = true)` is reached from client code as `O.Length(64)`.
// 2. A `name$default$n` getter reached through an *inserted* `apply` took the
//    receiver's qualifier: `G.H(4)` looked for `G$.apply$default$2`.
// 3. A trait nested in a member `object` and mixed into a class elsewhere: the
//    implementing class owes the trait's `$outer` accessor, and the object is
//    reached through the enclosing template's accessor rather than along the
//    `$outer` chain. Members of the class the trait *extends* are read off
//    `this` with a cast, not by walking out.
// 4. A concrete trait method whose *superclass* already overrides it -- at a
//    narrower erased descriptor, because an abstract type member was fixed --
//    got a mixin forwarder to the trait's own body, which then won over the
//    superclass's override. slick's `JdbcDatabaseDef.setupTransaction` was
//    shadowed by `BasicDatabaseDef`'s `= None`, so `.transactionally` ran with
//    autocommit on and rolled nothing back.

// 1. Constructor defaults on a companion module.
case class LtLength(length: Int, varying: Boolean = true)

object LtOuter {
  object LtInner {
    case class LtNested(length: Int, varying: Boolean = true)
  }
}

// The parameter's type names the class's own type parameter, so nsc infers the
// getter's result type from the body instead of declaring `F`: `None` does not
// conform to `F`. slick's `Comprehension[+Fetch <: Option[Node]]` is this.
// (Solving `F` *at a call site* that omits the argument is a separate thing
// this compiler still does not do -- see `docs/not-implemented.md` -- so the
// call below passes it; the getter's descriptor is what the test pins.)
case class LtComp[+F <: Option[Int]](name: String, fetch: F = None)

// A companion the source writes gets the getters too.
case class LtBox(a: Int, b: String = "d")
object LtBox { def make: LtBox = LtBox(1) }

// 2. A default reached through an inserted `apply`.
object LtF { def apply(a: Int, b: Int = 7): Int = a + b }
object LtG { object LtH { def apply(a: Int, b: Int = 3): Int = a * b } }

// 3. A trait nested in a member object of a trait, mixed into a class whose
//    own `$outer` is the profile, not the object.
trait LtComp2 {
  def quote(s: String): String = "[" + s + "]"
  class LtBuilder(val table: String) {
    def index: String = "base:" + table
    def columns: String = "cols"
  }
  object LtBuilder {
    trait LtUniqueAsConstraint extends LtBuilder {
      override def index: String = quote(table) + "/" + columns
    }
  }
}

trait LtProfile extends LtComp2 {
  class LtH2Builder(t: String) extends LtBuilder(t) with LtBuilder.LtUniqueAsConstraint
}

object LtH2 extends LtProfile

// 4. A superclass override at a narrowed descriptor, and an anonymous subclass
//    of it that mixes the trait in only through that superclass.
trait LtBase {
  type S <: AnyRef
  def setup(s: S): String = "base"
}

abstract class LtMid extends LtBase {
  type S = String
  override def setup(s: S): String = "mid:" + s
}

object Main {
  // Calling through `LtBase` uses the *wide* descriptor, which is where the
  // forwarder shadowed the override.
  def viaBase(b: LtBase)(x: b.S): String = b.setup(x)

  def main(args: Array[String]): Unit = {
    println(LtLength(64))
    println(LtOuter.LtInner.LtNested(40))
    println(LtComp("q", None))
    println(LtBox.make)
    println(LtF(2))
    println(LtG.LtH(4))
    println(new LtH2.LtH2Builder("x").index)
    val d = new LtMid {}
    println(viaBase(d)("x"))
    println(d.setup("y"))
  }
}
