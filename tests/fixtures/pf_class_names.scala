// The classes partial-function literals compile to, named and shaped as nsc
// names and shapes them: `$anonfun$<method>$<n>` after an owning method (a
// member or local `def`, a `lazy val`'s `lzycompute`, a default getter, a
// function literal's or by-name argument's method as `$nestedIn...`),
// `$anonfun$<n>` after a value or template statement, counters shared per
// unit and prefix with the methods function literals become, in the package
// of the enclosing class, extending `AbstractPartialFunction`.
package pk

object M {
  def byName(x: => Int): Int = x
  def m0(xs: Seq[Int]) = {
    val a = xs.map(x => x + 1)
    val b = xs.collect { case 1 => 2 }
    val c = xs.map(y => y)
    (a, b, c, xs.collect { case 3 => 4 })
  }
  def m0(s: String) = Seq(s).collect { case "a" => 1 }
  def q2(xs: Seq[Int]) = xs.collect { case 1 => 2 } ++ xs.map(x => x)
  def q3(xs: Seq[Int]) = xs.map(x => x) ++ xs.collect { case 1 => 2 }
  val f: PartialFunction[Int, Int] = { case 1 => 2 }
  val g: PartialFunction[String, String] = { case "x" => "y" }
  def nest(xs: Seq[Int]) = xs.map { x => Seq(x).collect { case 1 => 2 } }
  def nest2(xs: Seq[Int]) = xs.map { x => xs.map { y => Seq(y).collect { case 1 => 2 } } }
  def byNameArg(xs: Seq[Int]) = byName(xs.collect { case 1 => 2 }.size)
  def orElse(xs: Option[Int]) = xs.getOrElse(Seq(1).collect { case 1 => 5 }.head)
  def dflt(a: Int, xs: Seq[Int] = Seq(1).collect { case 1 => 7 }) = xs
  def lazyLocal(xs: Seq[Int]) = { lazy val l = xs.collect { case 1 => 1 }; l }
  lazy val lz = Seq(1).collect { case 1 => 9 }
  def locals(xs: Seq[Int]) = {
    def inner(z: Int) = Seq(z).collect { case 7 => 8 }
    def inner2 = Seq(1).collect { case 1 => 1 }
    inner(7) ++ inner2
  }
  def pfInPf(xs: Seq[Int]) = xs.collect { case 1 => Seq(1).collect { case 1 => 2 } }
  def +(xs: Seq[Int]) = xs.collect { case 1 => 11 }
}

class C(xs: Seq[Int]) {
  val r = xs.collect { case 1 => 12 }
  def op(ys: Seq[Int]) = ys.collect { case 1 => 13 }
}

trait T { def t(xs: Seq[Int]) = xs.collect { case 1 => 14 } }

object Main extends T {
  def main(args: Array[String]): Unit = {
    val xs = Seq(1, 3)
    println(M.m0(xs))
    println(List(M.m0("a"), M.q2(xs), M.q3(xs)))
    println(List(M.f.isDefinedAt(1), M.f.isDefinedAt(2), M.f(1), M.f.lift(2)))
    println(List(M.g("x"), M.g.applyOrElse("z", (s: String) => s + "!")))
    println(scala.util.Try(M.g("z")).failed.map(_.getClass.getName))
    println(List(M.nest(xs), M.nest2(xs), M.byNameArg(xs), M.orElse(None), M.dflt(0)))
    println(List(M.lazyLocal(xs), M.lz, M.locals(xs), M.pfInPf(xs), M + xs))
    println(List(new C(xs).r, new C(xs).op(xs), t(xs)))
    val shape = M.g.getClass
    println(List(shape.getSuperclass.getName, shape.getInterfaces.map(_.getName).toList))
    val bytes = new java.io.ByteArrayOutputStream
    new java.io.ObjectOutputStream(bytes).writeObject(M.g)
    val back = new java.io.ObjectInputStream(new java.io.ByteArrayInputStream(bytes.toByteArray))
      .readObject().asInstanceOf[PartialFunction[String, String]]
    println(back("x"))
  }
}
