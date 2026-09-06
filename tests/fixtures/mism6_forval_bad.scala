// A value definition must follow a generator in a for-comprehension. Both
// nsc and scala-rs reject this leading definition instead of inventing a
// stream to which it could belong.

object Main {
  def bad(ms: List[Int]): List[Int] = for {
    q = 1
    m <- ms
  } yield m + q

  def main(args: Array[String]): Unit = println(bad(1 :: Nil))
}
