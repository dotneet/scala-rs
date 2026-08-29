// Fifth `type mismatch` slice on slick: a trait that extends a function type
// as a SAM, a type parameter solved to the *caller's*, the type arguments an
// `extends` clause and a `new` leave out, a method whose only clause is
// implicit, `.apply` through an annotated type, and the collection a
// same-element-type transformation really returns.
//
// Every definition here is accepted by scalac 2.13.16; the expected output is
// what nsc prints for the same program.

import scala.collection.immutable.TreeMap

// -- a trait that extends a function type is a SAM --------------------------
// slick writes `trait CanBeQueryCondition[-T] extends (T => Rep[?])` and then
// `implicit val c: CanBeQueryCondition[Rep[Boolean]] = value => value`. The
// single abstract method is `Function1.apply`, inherited through a parent that
// is written structurally, so the SAM search has to read that parent back as
// the class -- and `apply`'s parameter is the *receiver's* `T`, which needs
// `FunctionN` to have type parameters at all.
trait Rep5[T] { def value: T }
class Lit5[T](val value: T) extends Rep5[T]
trait CanCond5[-T] extends (T => Rep5[?])
object CanCond5 {
  implicit val boolCol5: CanCond5[Rep5[Boolean]] = v => v
  implicit val bool5: CanCond5[Boolean] = v => new Lit5(v)
}

// -- a solution that is the caller's own type parameter ---------------------
// `mk5`'s `T` is solved from the lambda's *result*, which is `const5`'s `T`.
// The second inference pass threw away every solution that was a type
// parameter, so the result printed as `GR5[T] required GR5[T]`.
trait PR5 { def n: Int }
trait GR5[+T] extends (PR5 => T)
object GR5 {
  def mk5[T](implicit f: PR5 => T): GR5[T] = new GR5[T] { def apply(r: PR5): T = f(r) }
  def const5[T](value: T): GR5[T] = mk5(_ => value)
}

// -- the type arguments an `extends` clause leaves out ----------------------
// `class DerbySequenceDDLBuilder[T](seq: Sequence[T]) extends
// SequenceDDLBuilder.BuiltInSupport.OverrideActualStart(seq)`: the parent is
// applied to arguments that name the *subclass's* `T`, and nsc infers the
// parent's from them. Both sides printed `Sequence[T]` before.
class Seqn5[T](val v: T)
class BaseB5[T](val s: Seqn5[T]) { def g: T = s.v }
class DerivedB5[T](s2: Seqn5[T]) extends BaseB5(s2)

// -- the type arguments a `new` leaves out ----------------------------------
// The expected type names a *base* class, so `UnitRC5[R] <: RC5[R, Unit]`
// reads `R` off it. `TmRC5` gets `R` and `V` from the expected type and `U`
// from the constructor argument, so both sources have to be merged.
trait RC5[R, U] { def show: String }
class UnitRC5[R] extends RC5[R, Unit] { def show = "unit" }
class ProdRC5[R, U](xs: RC5[R, U]*) extends RC5[R, U] { def show = "prod" + xs.length }
class TmRC5[R, U, V](child: RC5[R, U], f: U => V) extends RC5[R, V] { def show = "tm" + child.show }
object RCs5 {
  def unit5[R]: RC5[R, Unit] = new UnitRC5
  def prod5[R, U](c: RC5[R, U]): RC5[R, U] = new ProdRC5(c)
  def tm5[R, U](c: RC5[R, U]): RC5[R, String] = new TmRC5(c, (u: U) => u.toString)
}

// -- a method whose only clause is implicit ---------------------------------
// `TreeMap.empty` is `[K: Ordering, V]: TreeMap[K, V]`. `V` sits in no
// implicit parameter, so the search alone cannot pin the parameters -- but the
// expected type can, and nsc runs `inferExprInstance` before the search. The
// whole method type used to stand as the value's own type.
object Impl5 {
  val empty5: TreeMap[Long, String] = TreeMap.empty
  def take5(m: TreeMap[Long, String]): Int = m.size
  def viaArg5: Int = take5(TreeMap.empty)
}

// -- `.apply` through an annotated type -------------------------------------
// slick binds `val (b, m: Map[…] @unchecked) = …` and then calls `m(f)`. An
// annotation says nothing about a type's members, so the `.apply` insertion
// has to look through it.
object Annot5 {
  def lookup5(b: Boolean): String = {
    val m: Map[String, (Int, String)] @unchecked =
      if (b) Map("a" -> ((2, "x"))) else Map.empty[String, (Int, String)]
    val (i, s) = m("a")
    s + i
  }
}

// -- the collection a same-element-type transformation returns --------------
// 2.13 declares `filterNot` / `++` / `take` as returning `C` (the receiver's
// own collection). The prelude cannot spell `C`, so `Vector[Phase].filterNot(p)`
// came back as the inherited `Seq` and `phases ++ ps` as an `IndexedSeq`.
// The conversions are deliberately left out: `v.toSeq` really is a `Seq`, and
// so is `TreeMap.filter`'s result here -- it erases to a *named* class, which
// codegen cannot narrow (see the README).
object Coll5 {
  val v5: Vector[Int] = Vector(1, 2, 3)
  val filtered5: Vector[Int] = v5.filterNot(_ == 2)
  val plus5: Vector[Int] = v5 ++ Seq(4)
  val taken5: Vector[Int] = v5.take(2)
  val rev5: Vector[Int] = v5.reverse
  val app5: Vector[Int] = v5 :+ 5
  val upd5: Vector[Int] = v5.updated(0, 9)
  val sorted5: Vector[Int] = v5.sortWith(_ > _)
  val set5: Set[Int] = Set(1, 2).filter(_ > 1)
  val asSeq5: Seq[Int] = v5.toSeq
}

// -- a factory's element type widened by the expected type ------------------
// `Set` and `Map` are invariant, so `Set(s): Set[Sym5]` is not a subtype
// question: the factory shortcut has to ask the expected type what the
// element is.
trait Sym5
class AnonSym5 extends Sym5 { override def toString = "anon" }
object Fac5 {
  def set5(s: AnonSym5): Set[Sym5] = Set(s)
  def map5(s: AnonSym5): Map[Sym5, Int] = Map(s -> 1)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(CanCond5.boolCol5(new Lit5(true)).value)
    println(CanCond5.bool5(false).value)
    println(GR5.const5("k").apply(new PR5 { def n = 0 }))
    println(new DerivedB5[Int](new Seqn5(3)).g)
    println(RCs5.unit5[String].show)
    println(RCs5.prod5(new UnitRC5[String]).show)
    println(RCs5.tm5(new UnitRC5[String]).show)
    println(Impl5.empty5.size + Impl5.viaArg5)
    println(Annot5.lookup5(true))
    println(Coll5.filtered5)
    println(Coll5.plus5)
    println(Coll5.taken5)
    println(Coll5.rev5)
    println(Coll5.app5)
    println(Coll5.upd5)
    println(Coll5.sorted5)
    println(Coll5.set5)
    println(Coll5.asSeq5)
    println(Fac5.set5(new AnonSym5))
    println(Fac5.map5(new AnonSym5))
  }
}
