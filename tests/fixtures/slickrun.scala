// Regression fixture for the `agent/slickrun` slice: the shapes that made
// scala-rs-compiled slick fail at *run* time even though every class file
// loaded. One file, many cases, in the order they were found.
import scala.collection.mutable

// (1) A nested `def` whose `match` binds names: those binders are bound, not
// free. Counting them as captures grew the lifted method extra parameters and
// made the enclosing trait declare capture accessors it could not implement.
class Nd
class CS(val sql: String) extends Nd
case class PS(cases: Seq[(Int => Boolean, Nd)], default: Nd) extends Nd
trait Comp {
  class Ext(tree: Nd, param: Int) {
    def result: String = {
      def findSql(n: Nd): String = n match {
        case c: CS => c.sql
        case PS(cases, default) =>
          findSql(cases.find { case (f, n) => f(param) }.map(_._2).getOrElse(default))
        case _ => "?"
      }
      findSql(tree)
    }
  }
}

// (2) An existential's skolem erases to its upper bound, so a member read
// through `Box[_ <: E]` owes a `checkcast` to `E`'s bound.
class Box[T](val value: T) { def get2: T = value }
abstract class Base { def tag: String }
class Impl(val tag: String) extends Base
class Holder[E <: Base](e: E) {
  lazy val shaped: Box[_ <: E] = new Box[E](e)
  def baseRow: E = shaped.value
  def baseRow2: E = shaped.get2
}

// (3) A case class's companion is not the case class: it must not run the
// `$init$` of the traits the class mixes in.
trait Marked { val mark: String = "m" }
final case class Ap(a: Int) extends Marked
final case class Ap2(a: Int, b: Int)(val extra: String) extends Marked

// (4) `3.compare(4)` goes through `RichInt`, not through an unmaterialised
// `Ordered` view.
class Counter {
  def length: Int = 3
  def lengthCompare(n: Int): Int = length.compare(n)
}

// (5) An auxiliary constructor of an *inner* class takes the enclosing
// instance too, and `this(...)` passes it on.
class Outer {
  class Inner(val a: Int, val b: String) {
    def this(a: Int) = this(a, "d")
    def show = a + b + Outer.this.tag
  }
  def tag = "!"
  def mk = new Inner(7)
}

// (6) `FunctionN.apply` erases to `Object`: a *tuple* result needs the cast
// its own method descriptor promises.
class T1 { override def toString = "T" }
class U1 { override def toString = "U" }

// (7) A trait `val` overridden by a narrower one in a derived trait: the base
// trait's mixin setter still has to exist (as a no-op) and the getter needs a
// bridge.
trait Opts { def name: String }
class BaseOpts extends Opts { def name = "base" }
class SubOpts extends BaseOpts { override def name = "sub" }
trait HasOpts { val opts: BaseOpts = new BaseOpts }
trait HasSubOpts extends HasOpts { override val opts: SubOpts = new SubOpts }

// (8) A default argument on a *trait* method needs its `name$default$n`
// getter on the interface and on every implementing class.
trait Defaulted {
  def label(prefix: String, upper: Boolean = false): String =
    if (upper) prefix.toUpperCase else prefix
}
class Defaults extends Defaulted

// (9) A `private` constructor the companion calls is emitted public, like nsc
// does (a constructor cannot be renamed).
final class Wrapped private (val n: Int) {
  private def this() = this(0)
  def bump = new Wrapped()
}
object Wrapped { def make(n: Int) = new Wrapped(n) }

// (10) A `case class` pattern that names only the first parameter list reads
// the constructor fields; there is no companion `unapply` to call.
final case class Tbl(schema: Option[String], name: String)(val extra: Int)

// (11) `if (c) e` with no `else` and a non-`Unit` branch value: both paths
// have to leave the same stack height.
class OneArmed {
  var seen = 0
  def touch(n: Int): String = { seen += n; "t" + n }
  def run(c: Boolean): Any = { if (c) touch(1); seen }
}

object Main {
  def g(n: T1)(f: T1 => (T1, U1)): (T1, U1) = n match { case null => null; case x => f(x) }

  def main(args: Array[String]): Unit = {
    val comp = new Comp {}
    println(new comp.Ext(new CS("q"), 1).result)
    println(new comp.Ext(PS(Seq(((i: Int) => i > 0, new CS("hit"))), new CS("miss")), 1).result)

    val h = new Holder[Impl](new Impl("hi"))
    println(h.baseRow.tag + h.baseRow2.tag)

    println(Ap(1).mark + Ap.apply(3) + Ap2(1, 2)("x").extra)

    println(new Counter().lengthCompare(5))
    println(new Counter().lengthCompare(3))

    val o = new Outer
    println(o.mk.show + new o.Inner(3, "x").show)

    println(g(new T1)(t => (t, new U1)))

    val so: HasOpts = new HasSubOpts {}
    println(so.opts.name)

    val d = new Defaults
    println(d.label("ab") + d.label("ab", upper = true))

    println(Wrapped.make(4).n + Wrapped.make(4).bump.n)

    val t = Tbl(Some("s"), "t")(9)
    val shown = t match { case Tbl(s, n) => s.getOrElse("-") + n }
    println(shown + t.extra)

    val oa = new OneArmed
    println(oa.run(true).toString + oa.run(false).toString)

    val buf = mutable.ArrayBuffer(3, 1, 2)
    println(buf.sortInPlace().mkString(","))
  }
}
