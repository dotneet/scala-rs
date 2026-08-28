// `sorted` は scala-library の `SeqOps` の default メソッドで、私有ランタイムの
// `List` classfile には無い。`--no-scala-library` では黙って通さず診断を出す。
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 3 :: 1 :: 2 :: Nil
    println(xs.sorted)
  }
}
