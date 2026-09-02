// `new C(a)(b)` is *one* constructor applied to two parameter lists, exactly
// as `extends A(1)(2)` is. Three things had to be true at once:
//
//  * the parser has to put `new` on the head of the `Apply` chain, not on its
//    outermost layer (`Apply(Apply(New(C), a), b)`);
//  * a case class's `copy()(x)` has to rebuild through that constructor -- the
//    companion's `apply` is only the same method when the companion is
//    synthetic, and a companion that declares any `apply` of its own gets no
//    synthetic one;
//  * a clause given explicitly must not be searched for again, and it is
//    weighed at the type arguments the `new` was written with.
//
// All three are slick's `slick/lifted/SimpleFunction.scala`:
// `final case class SimpleLiteral(name: String)(val buildType: Type)` with a
// companion `apply[T](name: String)(implicit tpe: TypedType[T])`, and
// `slick/lifted/Case.scala`'s `new TypedCase[B, P](…)(bType, om.liftedType(bType))`.

object Main {
  trait TT[T] { def label: String }
  trait BT[T] extends TT[T]

  // A curried case class whose companion declares an `apply` of its own.
  final case class Lit(name: String)(val buildType: Int) {
    def rebuild = copy()(buildType + 1)
    def renamed(n: String) = copy(name = n)(buildType)
  }
  object Lit {
    def apply(name: String, n: Int, m: Int): Lit = new Lit(name)(n + m)
  }

  // An explicit implicit clause.
  final class Ev[B, T](val s: String)(implicit val b: TT[B], val t: TT[T])
  // The same clause written as context bounds.
  final class Ctx[B: TT, T: TT](val s: String) {
    def labels(implicit b: TT[B], t: TT[T]) = b.label + "/" + t.label
  }

  // Named arguments belong to the clause that declares them, and the
  // clauses reach the constructor flattened.
  final class Multi(val a: Int)(val b: Int, val c: Int)

  // Not curried at all: `Fn` takes one parameter, so the second list is
  // `apply` on the instance. Folding the two lists together would build a
  // two-argument `Ov` instead -- silently, since `Ov` has such a constructor.
  final class Fn(val a: Int) { def apply(b: Int): Int = a + b }
  final class Ov(val a: Int) {
    def this(a: Int, b: Int) = this(a + b * 100)
    def apply(b: Int): String = a + "+" + b
  }

  implicit val ti: TT[Int] = new TT[Int] { def label = "int" }

  // `bt` is a `BT[B]`, and it only conforms to the constructor's `TT[B]`
  // once the class's own parameters are read at `[B, P]`.
  def mk[B, P](bt: BT[B], tp: TT[P]): Ctx[B, P] = new Ctx[B, P]("ctx")(bt, tp)
  def ev[B, P](b: TT[B], t: TT[P]): Ev[B, P] = new Ev[B, P]("ev")(b, t)

  def main(args: Array[String]): Unit = {
    val l = new Lit("a")(7)
    println(l.name + " " + l.buildType)
    val r = l.rebuild
    println(r.name + " " + r.buildType)
    println(l.renamed("z").name + " " + l.renamed("z").buildType)
    println(Lit("b", 3, 4).buildType)
    val bt = new BT[Int] { def label = "bt" }
    println(mk[Int, Int](bt, ti).s)
    println(ev[Int, Int](ti, ti).b.label)
    // The implicit clause left to the search still works.
    println(new Ctx[Int, Int]("cb").labels)
    val m = new Multi(1)(c = 3, b = 2)
    println(m.a + " " + m.b + " " + m.c)
    println(new Fn(1)(2))
    println(new Ov(1)(2))
    println(new Ov(1, 2).a)
  }
}
