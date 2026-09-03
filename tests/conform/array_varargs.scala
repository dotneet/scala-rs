// 参照配列とプリミティブ配列を混ぜる、可変長引数への `: _*` 展開。
case class Row(name: String, score: Int)

object Main {
  def render(parts: String*): String = parts.mkString("[", "|", "]")

  def total(xs: Int*): Int = xs.sum

  def main(args: Array[String]): Unit = {
    val rows = Array(Row("ada", 90), Row("bob", 72), Row("cy", 90))
    val names: Array[String] = rows.map(_.name)
    val scores: Array[Int] = rows.map(_.score)

    println(render(names: _*))
    println(total(scores: _*))
    println(rows.sortBy(r => (-r.score, r.name)).map(_.name).mkString(","))

    val labels = Array[Any]("n", 1, true)
    println(labels.mkString("<", ",", ">"))

    // `Array[Any]` の後で要素型を推論させる（記述子の持ち越し）。
    val ranks = Array(3, 1, 2)
    println(ranks.sorted.mkString(""))
    println(total(ranks: _*))

    println(names.zip(scores).map { case (n, s) => s"$n:$s" }.mkString(" "))
    println(scores.toSeq.sum)
    println(names.toSeq.mkString("+"))
  }
}
