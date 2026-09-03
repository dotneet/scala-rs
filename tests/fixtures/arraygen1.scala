// agent/arraygen: `Array` の codegen 3 件と、そこから出た 3 件。
// 全ケースを 1 ファイルに（実 scalac 1 回 1.8 秒）。
//
// 1) 明示型引数 `s.map[Int](f)` が as-seen-from を通らない（`map6` / `toArr`）。
// 2) `Array[Any](…)` と同じファイルの後続の `Array(3, 1, 2)` が壊れた記述子を
//    出して `VerifyError`。**宣言の順序が意味を持つ**ので `mixedFirst` を
//    `inferredLater` より前に置いてある。動かさないこと。
// 3) `Array[(Int, String)](…)` が `Object[]` を作って `ClassCastException`。
// 4) `f(arr: _*)` が Array を包まずに渡して `VerifyError`。
// 5) `Array[T]` の要素代入が `"[java/lang/Object".update` を出して
//    `ClassFormatError`（クラスがロードすらできない）。
// 6) `arr.clone()` / `arr :+ x` の記述子。
import scala.collection.immutable.HashSet
import scala.reflect.ClassTag

// 5 の別形。`agent/final1` がここを踏んで `Array.tabulate[R]` で回避していた。
final class CArr[+T](val xs: Seq[T]) {
  def toArr[R >: T: ClassTag]: Array[R] = {
    val out = new Array[R](xs.length)
    var i = 0
    while (i < xs.length) { out(i) = xs(i); i += 1 }
    out
  }
}

object Main {
  // 1) 明示型引数 + ジェネリック親からの継承メンバ。
  def map6(s: HashSet[String]): HashSet[Int] = s.map[Int](_.length)

  // 2) `Array[Any]` を含む宣言が先に来る。
  def mixedFirst(): String = Array[Any](1, "a").mkString(",")
  def inferredLater(): Int = Array(3, 1, 2).sum
  def inferredDouble(): String = Array(1.5, 2.5).mkString("/")

  // 3) 参照要素の推論。
  def pairs(): Array[(Int, String)] = Array[(Int, String)](1 -> "one", 2 -> "two")

  // 4) 可変長引数への `: _*` 展開。
  def render(parts: String*): String = parts.mkString("|")
  def total(xs: Int*): Int = xs.sum

  // 5) 抽象要素型の `Array[T]` に代入して読む。
  def repeat[T: ClassTag](x: T, n: Int): Array[T] = {
    val a = new Array[T](n)
    var i = 0
    while (i < n) { a(i) = x; i += 1 }
    a
  }

  // 6) `clone` と `:+` / `+:` / `updated`。
  def bump(a: Array[String], s: String): Array[String] = {
    val c = a.clone()
    (s +: c) :+ s
  }
  // 要素型が抽象なら配列自身も `Object` に潰れるので、`clone` も
  // `ScalaRunTime.array_clone` 経由（`"[I".clone` では嘘になる）。
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

    // `ClassTag` の要素クラスがそのまま `Array.apply` の生成に効く。
    println(Array[Array[Int]](Array(1), Array(2, 3)).map(_.length).mkString(""))
    println(Array[Option[Int]](Some(1), None).map(_.getOrElse(0)).sum)
  }
}
