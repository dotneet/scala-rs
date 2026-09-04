// agent/arraygen: three `Array` codegen bugs, and three more that fell out of them.
// Every case in one file (a real scalac run costs 1.8 s).
//
// 1) An explicit type argument `s.map[Int](f)` does not survive as-seen-from (`map6` / `toArr`).
// 2) An `Array(3, 1, 2)` later in the same file as an `Array[Any](…)` emitted a
//    broken descriptor and a `VerifyError`. **Declaration order matters here**,
//    so `mixedFirst` sits before `inferredLater`. Do not move them.
// 3) `Array[(Int, String)](…)` built an `Object[]` and threw `ClassCastException`.
// 4) `f(arr: _*)` passed the Array through unwrapped and threw `VerifyError`.
// 5) Storing into an element of `Array[T]` emitted `"[java/lang/Object".update`
//    and a `ClassFormatError` (the class would not even load).
// 6) The descriptors of `arr.clone()` / `arr :+ x`.
import scala.collection.immutable.HashSet
import scala.reflect.ClassTag

// A variant of 5. `agent/final1` hit this and worked around it with `Array.tabulate[R]`.
final class CArr[+T](val xs: Seq[T]) {
  def toArr[R >: T: ClassTag]: Array[R] = {
    val out = new Array[R](xs.length)
    var i = 0
    while (i < xs.length) { out(i) = xs(i); i += 1 }
    out
  }
}

object Main {
  // 1) An explicit type argument plus a member inherited from a generic parent.
  def map6(s: HashSet[String]): HashSet[Int] = s.map[Int](_.length)

  // 2) The declaration containing `Array[Any]` comes first.
  def mixedFirst(): String = Array[Any](1, "a").mkString(",")
  def inferredLater(): Int = Array(3, 1, 2).sum
  def inferredDouble(): String = Array(1.5, 2.5).mkString("/")

  // 3) Inference of a reference element type.
  def pairs(): Array[(Int, String)] = Array[(Int, String)](1 -> "one", 2 -> "two")

  // 4) `: _*` expansion into varargs.
  def render(parts: String*): String = parts.mkString("|")
  def total(xs: Int*): Int = xs.sum

  // 5) Store into and read back an `Array[T]` at an abstract element type.
  def repeat[T: ClassTag](x: T, n: Int): Array[T] = {
    val a = new Array[T](n)
    var i = 0
    while (i < n) { a(i) = x; i += 1 }
    a
  }

  // 6) `clone` and `:+` / `+:` / `updated`.
  def bump(a: Array[String], s: String): Array[String] = {
    val c = a.clone()
    (s +: c) :+ s
  }
  // With an abstract element type the array itself collapses to `Object` too, so
  // `clone` goes through `ScalaRunTime.array_clone` (`"[I".clone` would be a lie).
  def dup[T](a: Array[T]): Array[T] = a.clone()

  def main(args: Array[String]): Unit = {
    println(map6(HashSet("a", "bb", "ccc")).toList.sorted.mkString(","))
    println(mixedFirst())
    println(inferredLater())
    println(inferredDouble())

    val ps = pairs()
    println(ps.length)
    println(ps(0)._2)
    println(ps.map(_._1).sum)
    println(ps.mkString(";"))

    val names: Array[String] = Array("x", "y")
    val nums: Array[Int] = Array(4, 5, 6)
    println(render(names: _*))
    println(total(nums: _*))
    println(render(List("p", "q"): _*))

    println(repeat(3, 4).mkString(""))
    println(repeat("z", 2).mkString(""))
    println(repeat((1, "one"), 2).mkString(" "))
    println(new CArr[String](Seq("p", "q")).toArr[String].mkString("-"))
    println(new CArr[Int](Seq(1, 2)).toArr[Any].mkString("-"))

    println(bump(names, "!").mkString(""))
    println(names.mkString(""))
    println(nums.clone().updated(0, 9).mkString(","))
    println(dup(nums).sum)
    println(dup(names).mkString(""))

    // The `ClassTag`'s element class feeds straight into what `Array.apply` generates.
    println(Array[Array[Int]](Array(1), Array(2, 3)).map(_.length).mkString(""))
    println(Array[Option[Int]](Some(1), None).map(_.getOrElse(0)).sum)
  }
}
