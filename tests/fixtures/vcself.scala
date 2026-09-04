// agent/vcself: three defects that stood between `tests/slick_run.sh` and
// ten completed programs.
//
// 1. Inside a value class, a call to another of its own methods did not reach
//    the underlying value: the instance method passed `this` (a `VcOps`) to
//    `b$extension(String, int)`, and the `$extension` static re-boxed slot 0
//    with a `new VcOps(u)` before handing it on.
// 2. An erasure bridge whose implementation returns `Unit` returns `V`, and
//    the bridge itself owes a reference -- nsc pushes `BoxedUnit.UNIT`.
// 3. A `val` narrowed by an override in a *subclass* got no wide getter, so
//    the base class's own methods read the base's field.

final class VcOps(val s: String) extends AnyVal {
  def rep(n: Int): String = s * n
  // The instance method needs `aload_0; invokevirtual s()`; the `$extension`
  // static needs the bare slot 0.
  def bang(n: Int): String = rep(n) + "!"
  // Written `this`, same rule.
  def query(n: Int): String = this.rep(n) + "?"
  // A no-argument member is reached as an ordinary instance call, so the
  // `$extension` static does box -- which is what nsc does too.
  def dot: String = s + "."
  def twice: String = dot + dot
  // A lambda lifted out of a value class method captures the box and has to
  // unwrap it again for each `$extension` call it makes.
  def spread(n: Int): String = List(1, 2).map(i => rep(i + n)).mkString(",")
}

// A primitive underlying: the `$extension` static's slot 0 is a `double`, so
// the wrong receiver is a verifier error rather than a silent wrapper.
final class VcCel(val c: Double) extends AnyVal {
  def f: Double = c * 9.0 / 5.0 + 32.0
  def plus(n: Double): Double = c + n
  def warmer(n: Double): Double = plus(n) + 1.0
}

// 2. `SetParameter[-T] extends ((T, PositionedParameters) => Unit)`, the shape
//    slick's plain-SQL interpolation builds on. The erased
//    `apply(Object, Object)Object` bridge calls a method that returns `V`.
trait VcSink[-T] extends ((T, String) => Unit)

object VcSinkUnit extends VcSink[Unit] {
  def apply(none: Unit, pp: String): Unit = println("unit" + pp)
}

object VcSinkInt extends VcSink[Int] {
  def apply(n: Int, pp: String): Unit = println("int=" + n + pp)
}

// 3. slick's `JdbcStatementBuilderComponent.QueryBuilder` declares
//    `protected val quotedJdbcFns: Option[Seq[JdbcFunction]] = None` and
//    `H2Profile`'s subclass narrows it to `Some[Nil.type]`.
class VcBuilder {
  protected val quoted: Option[Seq[Int]] = None
  val plain: Seq[Int] = Seq(9)
  def show: String =
    (if (quoted.forall(_.contains(1))) "quote" else "bare") + "/" + plain.mkString
}

class VcH2Builder extends VcBuilder {
  override protected val quoted: Some[Nil.type] = Some(Nil)
  override val plain: List[Int] = List(1, 2)
}

object Main {
  def main(args: Array[String]): Unit = {
    val o = new VcOps("x")
    println(o.bang(3))
    println(o.query(2))
    println(o.twice)
    println(o.spread(1))
    println(new VcCel(100.0).f)
    println(new VcCel(1.5).warmer(2.5))

    val f: (Unit, String) => Unit = VcSinkUnit
    f((), "-a")
    val g: (Int, String) => Unit = VcSinkInt
    g(3, "-b")
    VcSinkUnit.apply((), "-c")

    println(new VcBuilder().show)
    println(new VcH2Builder().show)
    val b: VcBuilder = new VcH2Builder()
    println(b.show)
  }
}
