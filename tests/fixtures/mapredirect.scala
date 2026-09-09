object Warm {
  val factory = new scala.collection.MapFactory.Delegate[scala.collection.mutable.Map](scala.collection.mutable.Map)
  val warm = factory.apply[String, Int]()
  def main(args: Array[String]): Unit = {
    val entries = scala.collection.mutable.Map[String, Int]()
    entries("one") = 1
    println(entries.remove("one"))
  }
}
