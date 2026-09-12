// agent/libzero2: the inference and conformance roots that took scala/scala's
// own `src/library` from 41 errors to 4. Every case below was reduced from one
// library site and compiles under real scalac 2.13.16 with the same output.
import java.util.function.{BiConsumer, BiFunction, Consumer}

object Main {
  // 1. `Predef.$conforms[A]: A => A = <:<.refl`: a method's type argument read
  //    off an expected type that is a *function*, through a base class of the
  //    result (`=:=` reaches `Function1` two classes up).
  trait Sub[-A, +B] extends (A => B)
  trait Eq[A, B] extends Sub[A, B]
  object Eq { def refl[A]: Eq[A, A] = new Eq[A, A] { def apply(x: A): A = x } }
  def conforms[A]: A => A = Eq.refl
  def conformsSub[A]: Sub[A, A] = Eq.refl

  // 2. `TrieMap`'s `def this() = this(Hashing.default, Equiv.universal)`: a
  //    constructor self-call's arguments are typed against the formals of the
  //    one constructor the arity can mean.
  trait Hashing[T] { def hash(x: T): Int }
  object Hashing {
    final class Default[T] extends Hashing[T] { def hash(x: T) = x.hashCode }
    implicit def default[T]: Default[T] = new Default[T]
  }
  class Hashed[K](h: Hashing[K]) {
    def this(unused: Int, h: Hashing[K]) = this(h)
    def this() = this(Hashing.default)
    def of(k: K): Int = h.hash(k)
  }
  // ... and the curried form, `mutable.TreeMap`'s
  // `def this()(implicit ord: Ordering[K]) = this(RB.Tree.empty)(ord)`.
  class Tree[A, B]
  object Tree { def empty[A, B]: Tree[A, B] = new Tree[A, B] }
  class TM[K, V] private (tree: Tree[K, V])(implicit val ordering: Ordering[K]) {
    def this()(implicit ord: Ordering[K]) = this(Tree.empty)(ord)
    def t: Tree[K, V] = tree
  }

  // 3. `Factory.scala`'s `b ++= it`: a subclass method and an inherited one of
  //    the same name whose parameters name *unrelated* classes are two
  //    alternatives, not one member.
  trait Growable[A] {
    def addAll(xs: IterableOnce[A]): this.type = this
    final def ++=(xs: IterableOnce[A]): this.type = addAll(xs)
  }
  class Chars extends Growable[Char] {
    val seen = new StringBuilder
    override def addAll(xs: IterableOnce[Char]): this.type = { xs.iterator.foreach(seen += _); this }
    def ++=(s: String): this.type = { seen ++= s; this }
  }

  // 4. `StreamExtensions`'s eight `StreamShape` implicits: a **compound**
  //    receiver has the base types of its components, so a conversion's type
  //    parameter is solved from them -- and a conversion whose own parameter is
  //    compound is solved against the component that mentions them.
  trait Stepper[A]
  trait IntStepper extends Stepper[Int]
  trait EfficientSplit
  trait Shape[A, S]
  object Shape { implicit val int: Shape[Int, String] = new Shape[Int, String] {} }
  implicit class HasSeqStream[A](s: Stepper[A]) {
    def seqStream[S](implicit sh: Shape[A, S]): String = "seq"
  }
  implicit class HasParStream[A](s: Stepper[A] with EfficientSplit) {
    def parStream[S](implicit sh: Shape[A, S]): String = "par"
  }

  // 5. `BuildFrom`'s three `buildFrom*Ops`: a compound on the right of `<:` is
  //    every one of its components, even when the left side is an abstract
  //    constructor applied to arguments.
  trait MyMap[K, +V]
  trait MapOps2[K, +V, +CC[_, _], +C]
  trait BF[-From, -A, +C]
  implicit def bfMapOps[CC[X, Y] <: MyMap[X, Y] with MapOps2[X, Y, CC, _], K0, V0, K, V]
      : BF[CC[K0, V0] with MyMap[K0, V0], (K, V), CC[K, V] with MyMap[K, V]] =
    new BF[CC[K0, V0], (K, V), CC[K, V]] {}

  // 6. `Future.fold` / `Future.reduce`'s `sequence(futures)(ArrayBuffer, …)`: a
  //    closed argument position of an implicit candidate is a *conformance*
  //    question, read at that position's variance (`BF`'s `From` is `-`).
  trait Fac[+CC[_]]
  object Fac {
    implicit def toBuildFrom[A, CC[_]](f: Fac[CC]): BF[Any, A, CC[A]] = new BF[Any, A, CC[A]] {}
  }
  class AB[A]
  object AB extends Fac[AB]
  def sequence[A, CC[X] <: IterableOnce[X], To](in: CC[A])(implicit bf: BF[CC[A], A, To]): String = "seq"

  // 7. `Future.failed`'s `Promise.failed(exception).future`: a type parameter
  //    nothing in the call determines, carried out through a selection on the
  //    call's result and settled by the expected type.
  trait Fut[T] { def tag: String = "fut" }
  trait Prom[T] { def future: Fut[T] = new Fut[T] {} }
  object Prom { def failed[T](e: Throwable): Prom[T] = new Prom[T] {} }
  def failed[T](e: Throwable): Fut[T] = Prom.failed(e).future

  // 8. `FutureConvertersImpl`'s `whenCompleteAsync((t, e) => …)`: a SAM whose
  //    abstract method returns `Unit` discards the literal's value.
  def sams(): String = {
    val sb = new StringBuilder
    val bc: BiConsumer[String, Integer] = (s, i) => sb.append(s).append(i)
    bc.accept("a", 1)
    val c: Consumer[String] = s => sb.append(s).length
    c.accept("b")
    val r: Runnable = () => sb.append("c").length
    r.run()
    val bf: BiFunction[String, String, Int] = (a, b) => a.length + b.length
    s"${sb.toString}/${bf.apply("xx", "yyy")}"
  }

  // 9. `BigDecimal.sign`: the narrowest of three numeric conversions wins,
  //    because applicability is *weak* conformance.
  class BD(val tag: String)
  object BD {
    implicit def int2bd(i: Int): BD = new BD("int")
    implicit def long2bd(l: Long): BD = new BD("long")
    implicit def double2bd(d: Double): BD = new BD("double")
  }
  import BD._
  def signum: Int = 1
  def sign: BD = signum
  def signChar: BD = 'c'
  def signLong: BD = 7L

  // 10. `scala.Unit.box`: `Lscala/runtime/BoxedUnit;` in a **Java** class file
  //     means `BoxedUnit`, not `Unit`.
  def box(x: Unit): scala.runtime.BoxedUnit = scala.runtime.BoxedUnit.UNIT

  def main(args: Array[String]): Unit = {
    println(conforms[Int].apply(3))
    println(conformsSub[String].apply("x"))
    println(new Hashed[String]().of("ab") == "ab".hashCode)
    println(new TM[String, Int]().t != null)
    println((new Chars ++= List('x', 'y') ++= "z").seen.toString)
    println((new IntStepper with EfficientSplit).seqStream)
    println((new IntStepper with EfficientSplit).parStream)
    println(sequence(List(1, 2))(AB))
    println(failed[Int](new RuntimeException).tag)
    println(sams())
    println(s"${sign.tag} ${signChar.tag} ${signLong.tag}")
    println(box(()) != null)
  }

}
