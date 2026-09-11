// Specialization and primitive-typed generics: @specialized classes and
// methods, FunctionN over primitives, and generic code instantiated at Int,
// Double, Boolean and Unit.
object Main {
  class Box[@specialized(Int, Double) T](val v: T) { def get: T = v; def map[@specialized(Int) U](f: T => U): Box[U] = new Box(f(v)) }
  def sumSpec[@specialized(Int, Long) T](xs: Array[T])(implicit n: Numeric[T]): T = { var acc = n.zero; var i = 0; while (i < xs.length) { acc = n.plus(acc, xs(i)); i += 1 }; acc }
  class Acc[@specialized T](init: T) { var cur: T = init; def set(t: T): Unit = cur = t }
  trait Fn[@specialized(Int) A] { def apply(a: A): A }
  def applyTwice[A](f: A => A, a: A): A = f(f(a))
  def main(args: Array[String]): Unit = {
    val bi = new Box(3); val bd = new Box(1.5); val bs = new Box("s")
    println(bi.get + 1); println(bd.get * 2); println(bs.get + "!")
    println(bi.map(_ + 10).get); println(bd.map(_.toInt).get); println(bs.map(_.length).get)
    println(sumSpec(Array(1, 2, 3))); println(sumSpec(Array(1L, 2L))); println(sumSpec(Array(0.5, 0.25)))
    val a = new Acc(1); a.set(a.cur + 5); println(a.cur)
    val ab = new Acc(false); ab.set(!ab.cur); println(ab.cur)
    val au = new Acc(()); au.set(()); println(au.cur)
    val inc = new Fn[Int] { def apply(a: Int) = a + 1 }
    println(inc(41))
    val f1: Int => Int = _ * 3
    val f2: Double => Boolean = _ > 0.5
    val f3: (Int, Long) => Double = (i, l) => i + l + 0.5
    val f4: Int => Unit = i => println("unit fn " + i)
    val f5: () => Char = () => 'q'
    println(f1(3) + " " + f2(0.7) + " " + f3(1, 2L) + " " + f5())
    f4(9)
    println(applyTwice[Int](_ + 1, 0) + " " + applyTwice[String](_ + "x", "") + " " + applyTwice[Double](_ / 2, 8.0))
    val fs: List[Int => Int] = List(_ + 1, _ * 2, x => x * x)
    println(fs.map(_(5)))
    println((f1 andThen f1)(2) + " " + (f1 compose ((x: Int) => x - 1))(2))
    val m: Map[Int, Int => Int] = Map(1 -> f1)
    println(m(1)(7))
    val tup: (Int, Double) = (1, 2.0)
    println(tup._1 + tup._2)
    println(List(1, 2, 3).map(f1).sum + List(1.0, 2.0).map(_ * 2).sum)
  }
}
