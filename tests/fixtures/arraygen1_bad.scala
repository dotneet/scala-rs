// agent/arraygen の異常系。実 scalac 2.13.16 もこの 3 つを拒否する。
// 記述子を宣言から作るようにしたせいで、型の合わない `Array` 操作まで
// 通るようになっていないことの確認。
import scala.collection.immutable.HashSet
object Main {
  // 明示型引数は要素型と噛み合っていなければならない。
  def bad(s: HashSet[String]): HashSet[Int] = s.map[Int](x => x)
  // `Array[Int]` の要素に `String` は入らない。
  def worse(a: Array[Int]): Unit = a(0) = "x"
  // `Array` は可変長引数の要素型まで自由にはならない。
  def worst(a: Array[Int]): String = render(a: _*)
  def render(parts: String*): String = parts.mkString
  def main(args: Array[String]): Unit = ()
}
