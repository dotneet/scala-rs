// Fourth `type mismatch` slice on slick: a type alias completed before the
// signature pass, a compound type conforming to an applied abstract member,
// `Map` as a function, `map`'s result on an `IndexedSeq`, and a stable
// identifier matched against a scrutinee whose type is not yet known.
//
// Every definition here is accepted by scalac 2.13.16; the expected output is
// what nsc prints for the same program.

// -- an alias whose right-hand side is an imported name ---------------------
// The alias is completed on demand while `Simple4`'s parent clause is resolved,
// which is before the signature pass reaches it. Only the namer had seen it by
// then, and the namer records no scopes, so the stack rebuilt from the owner
// chain carried the enclosing templates' members but not this unit's imports:
// `Fixed4` resolved to nothing and `PA4` was `<error>` for the rest of the run.
package hidden4 {
  trait Eff4
  object Eff4 { trait Schema4 extends Eff4 }
  trait Fixed4[+R, -E <: Eff4] { def label: String }
}

package app4 {
  import hidden4.{Eff4, Fixed4}

  trait Comp4 {
    type PA4[+R, -E <: Eff4] = Fixed4[R, E]
    abstract class Simple4[+R](n: String) extends PA4[R, Eff4] {
      def label: String = n
    }
    // `-E` is contravariant, so a `PA4[Unit, Eff4]` is a `PA4[Unit, Schema4]`.
    def create: PA4[Unit, Eff4.Schema4] = new Simple4[Unit]("schema.create") {}
  }
  object Comp4 extends Comp4
}

// -- a compound type conforms through one of its own parents ----------------
// `B4[R] with M4[R]` has to conform to `A4[R] with M4[R]`. The right-hand
// `M4[R]` is an *abstract* member applied to arguments: nothing on the right
// can settle the question, so the left side's own rule has to run.
trait A4[+R]
trait B4[+R] extends A4[R]
trait P4 {
  type M4[+R] <: A4[R]
  type N4[+R] <: A4[R] with M4[R]
}
trait Q4 extends P4 {
  type M4[+R] <: B4[R]
  type N4[+R] <: B4[R] with M4[R]
}

// -- `Map[K, V]` is a `K => V`, and a `FunctionN` class is a function -------
object Fn4 {
  val m: Map[String, Int] = Map("a" -> 1, "b" -> 2)
  val asFn: String => Int = m
  val pf: PartialFunction[String, Int] = { case "a" => 10 }
  val pfAsFn: String => Int = pf
}

// -- `map` keeps the receiver's own collection ------------------------------
// `IndexedSeq` does not redeclare `map`; the declaration it inherits says
// `Seq[B]`, but the real signature returns the receiver's type constructor.
object Coll4 {
  val idx: IndexedSeq[String] = IndexedSeq(1, 2, 3).map(_.toString)
  val sq: Seq[String] = Seq(1, 2).map(_.toString)
  val vec: Vector[String] = Vector(1, 2).map(_.toString)
  // `Range` really does map to an `IndexedSeq`: it has no type parameter of
  // its own, so the declared result still wins.
  val rng: IndexedSeq[Int] = (1 to 3).map(_ * 2)
}

// -- a stable identifier against a scrutinee that is still abstract ---------
// `T` could be `Byte`, and the pattern is only an `==` at run time, so a
// scrutinee that still names a type parameter rules nothing out.
trait ST4[T] { def name: String }
class Num4[T](val name: String) extends ST4[T]
object Num4 {
  val byteType: Num4[Byte] = new Num4[Byte]("byte")
  val intType: Num4[Int] = new Num4[Int]("int")
}
object Pat4 {
  def widthOf[T](t: ST4[T]): Int = t match {
    case Num4.byteType => 1
    case Num4.intType  => 4
    case _             => 0
  }
}

// -- `type Self >: this.type` read from a subclass -------------------------
// The lower bound is written in `Nd4`'s vocabulary, so seen from `Nullary4` it
// is `Nullary4.this.type`: returning `this` where a `Self` is wanted is right.
// Only a `this` tree gets this -- `mism4_bad` keeps the other direction.
trait Nd4 {
  type Self >: this.type <: Nd4
  def name: String
  def mapCh(f: Nd4 => Nd4): Self
}
trait Tagged4 { def tag: Int }
class Nullary4(val name: String) extends Nd4 with Tagged4 {
  type Self = Nullary4
  def tag: Int = 0
  final def mapCh(f: Nd4 => Nd4): Self = this
  def keep: Self with Tagged4 = this
}
class Unary4(val name: String, val child: Nd4) extends Nd4 {
  type Self = Unary4
  def rebuild(c: Nd4): Self = new Unary4(name, c)
  final def mapCh(f: Nd4 => Nd4): Self = {
    val c2 = f(child)
    val n: Self = if (c2 eq child) this else rebuild(c2)
    n
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(app4.Comp4.create.label)
    println(Fn4.asFn("b"))
    println(Fn4.pfAsFn("a"))
    println(Coll4.idx)
    println(Coll4.sq)
    println(Coll4.vec)
    println(Coll4.rng)
    println(Pat4.widthOf(Num4.byteType))
    println(Pat4.widthOf(Num4.intType))
    println(Pat4.widthOf(new Num4[Long]("long")))
    val leaf = new Nullary4("leaf")
    println(leaf.mapCh(identity).name)
    println(leaf.keep.tag)
    println(new Unary4("up", leaf).mapCh(identity).name)
    println(new Unary4("up", leaf).mapCh(_ => new Nullary4("other")).child.name)
  }
}
