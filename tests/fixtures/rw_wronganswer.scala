// Programs that compiled, verified and ran, and printed the wrong answer.
//
// Every block below is a corpus `run` test that failed with
// `output-mismatch` or a failed `assert` -- the class of defect no other
// check in the battery can see, because the classfile is well formed and the
// JVM is happy with it. One file, many cases: a dual-run against real scalac
// costs 1.8 s per file (`.agent-brief.md`).

import Int.MaxValue

// (1) More than 32 `lazy val`s in one class. `bitmap$0` is a single `int`,
// and the 33rd bit used to be `1 << 32`, which the JVM reduces to `1 << 0`:
// forcing the first `lazy val` made every later one report itself
// initialised and hand back the field's default. `run/t3038c`.
class ManyLazies {
  lazy val a01 = 1; lazy val a02 = 2; lazy val a03 = 3; lazy val a04 = 4
  lazy val a05 = 5; lazy val a06 = 6; lazy val a07 = 7; lazy val a08 = 8
  lazy val a09 = 9; lazy val a10 = 10; lazy val a11 = 11; lazy val a12 = 12
  lazy val a13 = 13; lazy val a14 = 14; lazy val a15 = 15; lazy val a16 = 16
  lazy val a17 = 17; lazy val a18 = 18; lazy val a19 = 19; lazy val a20 = 20
  lazy val a21 = 21; lazy val a22 = 22; lazy val a23 = 23; lazy val a24 = 24
  lazy val a25 = 25; lazy val a26 = 26; lazy val a27 = 27; lazy val a28 = 28
  lazy val a29 = 29; lazy val a30 = 30; lazy val a31 = 31; lazy val a32 = 32
  lazy val a33 = 33; lazy val a34 = 34; lazy val a35 = 35; lazy val a36 = 36
  def all: List[Int] = List(a01, a02, a03, a04, a05, a06, a07, a08, a09, a10,
    a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24,
    a25, a26, a27, a28, a29, a30, a31, a32, a33, a34, a35, a36)
}

// (2) A `private` member of a trait is not inherited (SLS 5.2). Whichever
// parent the member traversal reached first used to answer, so the one that
// is not a member at all could win. `run/t7475b`.
trait PrivA { private val x = 1 }
trait PubB { val x = 2 }
trait Mix1 extends PubB with PrivA { def get = x }
trait Mix2 extends PrivA with PubB { def get = x }

// (3) A case class's `equals` ends with `that.canEqual(this)`, which is what
// lets a subclass refuse an equality the superclass's fields accept.
// `run/caseClassEquality`.
case class CC1(x: Int)
class CCSub(x: Int) extends CC1(x) {
  override def canEqual(other: Any) = other.isInstanceOf[CCSub]
  override def equals(other: Any) = other match {
    case o: CCSub => x == o.x
    case _        => false
  }
}
class CCPlain(x: Int) extends CC1(x)

// (4) `@volatile` and `final` on a trait's `val` reach the class that mixes
// it in. `run/t8087`, `run/trait_fields_volatile`, `run/trait_fields_final`,
// `run/trait_fields_bytecode`.
trait Flags {
  @volatile private var flag = false
  final val fin: Int = 7
  val plain: Int = 8
  def touch(): Unit = flag = !flag
}
class Flagged extends Flags

// (5) `@scala.annotation.varargs` adds the Java-shaped entry point.
// `run/t5125`, `run/t5125b`.
class VarargsHost {
  @scala.annotation.varargs
  def f(values: String*): Int = values.length
  @scala.annotation.varargs
  def g(n: Int, values: Int*): Int = n + values.length
}

// (6) An empty repeated argument is `Nil`, not an empty `ArraySeq`; the
// callee can print what it was handed. `run/t5966`.
object Repeat { def take(xs: AnyRef*): Seq[AnyRef] = xs }

object Main {
  import java.lang.reflect.Modifier

  def main(args: Array[String]): Unit = {
    val m = new ManyLazies
    println(m.all.sum)
    println(m.all.drop(32))

    println((new Mix1 {}).get)
    println((new Mix2 {}).get)

    // (3) canEqual
    println(CC1(5) == new CCPlain(5))
    println(CC1(5) == new CCSub(5))
    println(new CCSub(5) == CC1(5))

    // (4) trait field flags, read back through reflection
    val flagged = classOf[Flagged]
    println(flagged.getDeclaredFields
      .filter(_.getName.contains("flag"))
      .map(f => Modifier.isVolatile(f.getModifiers))
      .mkString(","))
    println(Modifier.isFinal(flagged.getDeclaredMethod("fin").getModifiers))
    println(Modifier.isFinal(flagged.getDeclaredMethod("plain").getModifiers))

    // (5) the Java overload exists beside the Scala one
    println(classOf[VarargsHost].getDeclaredMethods
      .filter(mm => mm.getName == "f" || mm.getName == "g")
      .map(_.toString).sorted.mkString("\n"))
    println(new VarargsHost().f("a", "b"))
    println(new VarargsHost().g(1, 2, 3))

    // (6) empty repeated argument
    println(Repeat.take())
    println(Repeat.take("a"))

    // (7) `'sym` is a `scala.Symbol`, not its name, and `Symbol.apply`
    // interns. `run/t4560`, `run/t4601`.
    println("" + 'blubber)
    println(('blubber: AnyRef) eq ('blubber: AnyRef))

    // (8) A stable identifier pattern whose name came from an import of a
    // *classfile* member is a comparison, not a fresh binding. The name
    // resolved to the accessor -- a nullary method -- and `uncurry`
    // eta-expanded the pattern into `() => Int`, which the pattern
    // generator does not recognise, so no test was emitted at all and the
    // first case matched everything.
    println(5 match { case MaxValue => "wrong"; case _ => "ok" })
    println(MaxValue match { case MaxValue => "ok"; case _ => "wrong" })
  }
}
