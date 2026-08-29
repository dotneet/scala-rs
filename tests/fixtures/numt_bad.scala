// 数値の塔で scalac が拒否するもの。どれも診断が出ないといけない。
object Main {
  def takeB(x: Byte): Int = x.toInt

  def main(args: Array[String]): Unit = {
    // 縮小変換は暗黙には起きない（`toByte` を書かないといけない）。
    val i = 300
    val b: Byte = i

    // 定数でも範囲外なら narrow できない（SLS 6.26.1）。
    val b2: Byte = 300

    // Byte のパラメータに範囲外の定数は渡せない（`takeB(3)` は SLS 6.26.1 で通る）。
    println(takeB(300))

    // Boolean には toX が無い。
    println(true.toInt)

    // Unit にも無い。
    println(().toByte)

    // 逆向きの弱適合は無い（Double から Int へは落ちない）。
    val n: Int = 1.5

    // Char へ縮小するのも暗黙には起きない。
    val c: Char = i
  }
}
