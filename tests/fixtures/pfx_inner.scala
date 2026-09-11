// Inner classes of classes seen through a prefix (`crates/typer/src/prefix.rs`):
// the enclosing instance's type arguments reach the members of `o.In`, and
// `p.In` / `Outer#In` / `Outer.this.In` are told apart where nsc tells them
// apart. Every case here is accepted by scalac 2.13.16 and the output is
// compared with scalac's.

// Outer type arguments through an inner class returned by a method (cats'
// `syntax/semigroupal.scala`): `new B[X].m(1).n(x)` reads `n(z: T)` at `T = X`.
class X { override def toString = "X" }
class B[T] { def m[A](a: A) = new B1(a); class B1[A0](a0: A0) { def n(z: T) = z } }

// An inner class extending a parent written in the enclosing class's
// vocabulary (`HashMap.keySet`): `new KeySet` inside `HM[K]` is a `MySet[K]`.
trait MySet[K] { def size: Int }
trait MapOps[K] { class KeySet extends MySet[K] { def size = 7 } }
class HM[K] extends MapOps[K] { def ks: MySet[K] = new KeySet }

// `new o.In(...)` on `o: Outer[String]`: the constructor and the members take
// `String` for `T`.
class Outer[T](val x: T) {
  class In(val t: Option[T]) { def get: T = x; def pair: (T, T) = (x, x) }
  class Mid { class Deep { def get: T = x }; def deep = new Deep }
  type I = In
  def in = new In(None)
  def mid = new Mid
  def mk: I = new In(Some(x))
  def use(i: I): T = i.get
  def ins: List[In] = List(new In(None), new In(None))
  case class Rec(a: Int, t: T)
  def rec: Rec = Rec(1, x)
}

// Members of an inner class of a generic trait, reached through an object
// that extends it (`IntIsIntegral.mkNumericOps(6) / 2`, with the trait in
// source).
trait Num[T] {
  def plus(a: T, b: T): T
  class Ops(lhs: T) { def +(rhs: T): T = plus(lhs, rhs) }
  implicit def mkOps(x: T): Ops = new Ops(x)
  def mk(x: T): Ops = new Ops(x)
}
object IntNum extends Num[Int] { def plus(a: Int, b: Int) = a + b }

// Paths: `this.In`, a self alias, an inherited inner class, dependent
// method types, and `Outer#In` as the supertype of every `p.In`.
class Base { class In { def n = 6 }; def mk: In = new In; def id(x: In): this.In = x }
class Sub extends Base { def use: In = mk; def use2(i: In): In = i; def use3: this.In = new In }
trait SelfAlias { self =>
  class In { def n = 1 }
  def f: self.In = new In
  def g(i: In): self.In = i
}
class Plain { class In { def n = 4 }; class Other; def mk = new In
  def test(x: AnyRef): Int = x match { case i: In => i.n; case _: Other => 0; case _ => -1 }
}
object Dep { def f(o: Plain)(i: o.In): Int = i.n }

// An inner trait as an implicit's target, reached through a context bound,
// and an inner class's parents seen through the prefix by implicit search.
trait Types {
  trait JT[U] { def name: String }
  trait BCT[U] extends JT[U]
  def need[U](implicit j: JT[U]): String = j.name
  def base[U: BCT]: String = need[U]
}
object StrTypes extends Types { implicit val bs: BCT[String] = new BCT[String] { def name = "str" } }
trait AbstractTable[T] { def t: T }
trait FlatShapeLevel
trait Shape[Level, M, U, P] { def name: String }
object Shape {
  implicit def tableShape[Level <: FlatShapeLevel, T, C <: AbstractTable[_]](implicit ev: C <:< AbstractTable[T]): Shape[Level, C, T, C] =
    new Shape[Level, C, T, C] { def name = "table" }
}
object Q { def needShape[C, T](c: C)(implicit s: Shape[FlatShapeLevel, C, T, C]): String = s.name }
trait Comp { self: Prof =>
  class Repos extends AbstractTable[Int] { def t = 1 }
  lazy val Repos = new Repos
  def use: String = Q.needShape(Repos)
}
trait Prof extends Comp
object Profile extends Prof

// Overriding with prefixed parameter types: `p.S1` against `p.S1`, and the
// same inner class through `this`.
class P {
  trait S1
  class S0 { def n = 9 }
  lazy val p = new P
  trait S2 { def f(x: p.S1): Int }
  trait S3 { def g(x: S0): Int }
  object O extends S3 { def g(x: S0) = x.n }
}
class P2 extends P { object O2 extends S2 { def f(x: p.S1) = 5 } }

// An inner class of a generic class instantiated in a subclass's method with
// explicit type arguments and a mixin (slick's `ReturningInsertActionComposerImpl`).
trait CompX {
  class Impl[U, QR, RU](val k: Int, val mux: (U, QR) => RU) { def run(u: U, q: QR): RU = mux(u, q) }
  trait All[U] { def all: Int = 2 }
  def create[U, QR, RU](k: Int, mux: (U, QR) => RU): Impl[U, QR, RU] = new Impl[U, QR, RU](k, mux)
}
class SubX extends CompX {
  override def create[U, QR, RU](k: Int, mux: (U, QR) => RU): Impl[U, QR, RU] = new Impl[U, QR, RU](k, mux) with All[U]
}

// A higher-kinded outer: `new SB[F] |@| fa |@| fb` (cats' `SemigroupalBuilder`).
class SB[F[_]] {
  def |@|[A](a: F[A]) = new SB1[A](a)
  class SB1[A](a: F[A]) { def |@|[Z](z: F[Z]) = new SB2[A, Z](a, z) }
  class SB2[A, Z](a: F[A], z: F[Z]) { def both: (F[A], F[Z]) = (a, z) }
}

object Main {
  def mkSB[F[_], A, B](fa: F[A], fb: F[B]): SB[F]#SB2[A, B] = new SB[F] |@| fa |@| fb
  def useIn(o: Outer[String])(i: o.In): String = i.get
  def main(args: Array[String]): Unit = {
    val x = new X
    println(new B[X].m(1).n(x))
    println(new HM[Int].ks.size)
    val hm = new HM[Int]
    val ks: MySet[Int] = new hm.KeySet
    println(ks.size)

    val o = new Outer("str")
    val i = new o.In(Some("s"))
    println(i.t.get.length + i.get.length + i.pair._2.length)
    println(useIn(o)(i) + o.use(o.mk) + o.ins.map(_.get.length).sum)
    val j: o.In = o.in
    val k: Outer[String]#In = j
    val a: o.I = o.mk
    val b: o.In = a
    println(k.get + b.get)
    val m = o.mid
    val d: String = m.deep.get
    val dd: m.Deep = new m.Deep
    println(d + dd.get + o.mid.deep.get.length)
    val r = o.rec
    val s: String = r.t
    val r3: o.Rec = new o.Rec(3, "z")
    r3 match { case o.Rec(n, t) => println(s + n + t + r.a) }

    println(IntNum.mk(6) + 2)
    locally { import IntNum._; println((6: Int) + 2) }

    val sb = new Sub
    val si: sb.In = sb.use
    println(sb.use2(si).n + sb.use3.n + sb.id(si).n)
    val sa = new SelfAlias {}
    val sai: sa.In = sa.f
    println(sa.g(sai).n)
    val pl = new Plain
    val pi: pl.In = pl.mk
    val pj: Plain#In = pl.mk
    val pw = if (pi.n > 0) pl.mk else pl.mk
    val pv: pl.In = pw
    println(pl.test(pi) + pl.test(new pl.Other) + Dep.f(pl)(new pl.In) + pj.n + pv.n)

    println(StrTypes.base[String] + Profile.use + Q.needShape(Profile.Repos))
    val p2 = new P2
    println(p2.O2.f(new p2.p.S1 {}) + p2.O.g(new p2.S0))
    val sx = new SubX
    val impl = sx.create[Int, String, Int](1, (u, q) => u + q.length)
    println(impl.run(2, "abc") + impl.k)
    val (fa, fb) = mkSB(Option(1), Option("s")).both
    println(fa.get + fb.get.length)
  }
}
