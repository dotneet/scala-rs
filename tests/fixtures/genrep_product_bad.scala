// `TupleN extends Product with Serializable` is linked from the real
// scala-library jar. The private runtime (`--no-scala-library`) has neither
// interface and its `scala/Tuple2` implements neither, so this must still be
// diagnosed there rather than quietly accepted.
object Main {
  def main(args: Array[String]): Unit = {
    val p: Product = (1, "x")
    println(p.productArity)
  }
}
