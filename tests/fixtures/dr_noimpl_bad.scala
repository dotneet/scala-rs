// implicit しか取らないメソッドは値ではない。埋められないなら型エラーで、
// 黙って eta 展開して関数値にしてはいけない（以前は
// `println(List(Some(1), None, Some(3)).flatten)` が
// `Main$$$anonfun$0@7a765367` を印字していた）。
// 実 scalac も「could not find implicit value for parameter m」で落ちる。
trait Marker[A] { def tag: String }

object Main {
  def widget[A](implicit m: Marker[A]): String = m.tag

  def main(args: Array[String]): Unit = {
    println(widget)
  }
}
