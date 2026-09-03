// agent/setmap の異常系。実 scalac 2.13.16 もこの 2 つを拒否する
// （`Array[Int]` は `Seq[String]` にならないし、`collection.Map` に
// `noSuchLookup` は無い）。包み込みを足したせいで何でも通るように
// なっていないことの確認。
object Main {
  def bad(a: Array[Int]): Seq[String] = a
  def worse(m: collection.Map[String, Int]): Int = m.noSuchLookup("k")
  def main(args: Array[String]): Unit = ()
}
