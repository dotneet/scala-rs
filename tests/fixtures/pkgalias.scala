// Type aliases declared by a jar package object. They exist only in the
// `ScalaSignature` pickle -- `scala/package$.class` declares no member for
// them -- so resolving them at all means reading that pickle.
object Main {
  // `scala.NoSuchElementException = java.util.NoSuchElementException`
  def boom(): Unit = throw new NoSuchElementException("gone")

  // `scala.Throwable`, and the aliases for the java.lang exceptions.
  def name(t: Throwable): String = t.getClass.getName

  // A parameterized alias: `scala.IterableOnce[A] = collection.IterableOnce[A]`.
  def count(xs: IterableOnce[Int]): Int = xs.iterator.length

  def main(args: Array[String]): Unit = {
    val caught: String =
      try {
        boom()
        "none"
      } catch {
        case e: NoSuchElementException => e.getMessage
      }
    println(caught)
    println(name(new UnsupportedOperationException("nope")))
    println(name(new IllegalArgumentException("bad")))
    println(count(List(1, 2, 3)))
    val e: Exception = new RuntimeException("r")
    println(e.getMessage)
    val s: Seq[Int] = Seq(4, 5)
    println(s.sum)
  }
}
