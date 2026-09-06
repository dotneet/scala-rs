object Main {
  val factory = scala.collection.Factory
  val lazyList = scala.collection.immutable.LazyList

  def main(args: Array[String]): Unit =
    println(factory.getClass.getName + ":" + lazyList.getClass.getName)
}
