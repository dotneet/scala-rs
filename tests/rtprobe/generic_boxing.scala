// Generic code over primitives: boxing on entry, unboxing on exit, generic
// containers of Unit/Char/Boolean, identity preservation, and numeric
// results of generic methods used in arithmetic.
object Main {
  def id[T](t: T): T = t
  def pair[A, B](a: A, b: B): (A, B) = (a, b)
  def twice[T](t: T)(f: T => T): T = f(f(t))
  def pick[T](c: Boolean, a: T, b: T): T = if (c) a else b
  class Cell[T](var v: T) { def get: T = v; def set(t: T): Unit = v = t }
  def sumAll[N](xs: List[N])(implicit num: Numeric[N]): N = xs.foldLeft(num.zero)(num.plus)
  def maxBy[T, K: Ordering](xs: List[T])(k: T => K): T = xs.maxBy(k)
  def main(args: Array[String]): Unit = {
    val i: Int = id(41) + 1; val d: Double = id(1.5) * 2; val c: Char = id('a'); val b: Boolean = !id(false); val l: Long = id(1L) << 40
    println(s"$i $d $c $b $l")
    println(id(()) + " " + id(null: String) + " " + id(3.toByte) + " " + id(4.toShort) + " " + id(2.5f))
    val p = pair(1, 'x')
    println(p._1 + 1 + " " + (p._2 + 1) + " " + p)
    println(twice(3)(_ * 3) + " " + twice("a")(_ + "b") + " " + twice(1.0)(_ / 4) + " " + twice('a')(x => (x + 1).toChar))
    println(pick(true, 1, 2) + pick(false, 10, 20) + " " + pick(true, "s", "t") + " " + pick(false, 1.5, 2.5))
    val ci = new Cell(1); ci.set(ci.get + 1); println(ci.get * 10)
    val cc = new Cell('a'); cc.set((cc.get + 2).toChar); println(cc.get)
    val cb = new Cell(true); cb.v = !cb.v; println(cb.get)
    val cu = new Cell(()); println(cu.get)
    println(sumAll(List(1, 2, 3)) + " " + sumAll(List(1.5, 2.5)) + " " + sumAll(List(10L, 20L)) + " " + sumAll(List(BigInt(1), BigInt(2))))
    println(maxBy(List("aa", "b", "ccc"))(_.length) + " " + maxBy(List(3, -9, 5))(math.abs))
    val xs: List[Any] = List(1, 2.0, 'c', true, (), 3L)
    println(xs.map(x => x.getClass.getSimpleName).mkString(","))
    val boxedSum = xs.collect { case n: Int => n; case n: Long => n.toInt }.sum
    println(boxedSum)
    val anyInt: Any = 5
    val back = anyInt.asInstanceOf[Int] + 1
    println(back)
    val m = Map(1 -> 'a', 2 -> 'b')
    println(m(1).toInt + m(2))
    val arr: Array[Any] = Array(1, "s")
    arr(0) = arr(0).asInstanceOf[Int] + 1
    println(arr.toList)
    val opt: Option[Int] = Some(5)
    val sum = opt.getOrElse(0) + opt.map(_ * 2).getOrElse(0)
    println(sum)
    val fn: Int => Double = _ / 2.0
    println(List(1, 2, 3).map(fn).sum)
    val listOfUnit = List((), ())
    println(listOfUnit)
    val f0: () => Int = () => 7
    println(f0() + f0())
  }
}
