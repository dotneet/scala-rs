// `--no-scala-library` には `ClassTag` も `ScalaRunTime.array_update` も
// 無いので、抽象要素型の `Array[T]` を作って埋める形は**診断**になること。
// 実 jar を渡せば同じファイルが通る（`arraygen.rs` の 2 本で両方を留める）。
import scala.reflect.ClassTag
object Main {
  def repeat[T: ClassTag](x: T, n: Int): Array[T] = {
    val a = new Array[T](n)
    var i = 0
    while (i < n) { a(i) = x; i += 1 }
    a
  }
  def main(args: Array[String]): Unit = println(repeat(1, 2).length)
}
