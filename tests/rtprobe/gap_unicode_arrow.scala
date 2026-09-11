// scala-rs rejects: the deprecated unicode arrow `→` in ArrowAssoc.
object Main {
  def main(args: Array[String]): Unit = println(("k" → "v") + " " + Map(1 → 2))
}
