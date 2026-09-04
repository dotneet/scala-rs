// The rejection side of agent/setmap. Real scalac 2.13.16 rejects these two too
// (`Array[Int]` is not a `Seq[String]`, and `collection.Map` has no
// `noSuchLookup`). Confirms that adding the wrapping did not start letting
// anything through.
object Main {
  def bad(a: Array[Int]): Seq[String] = a
  def worse(m: collection.Map[String, Int]): Int = m.noSuchLookup("k")
  def main(args: Array[String]): Unit = ()
}
