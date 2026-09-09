object Bad {
  val wrong: scala.collection.mutable.Map[Int, String] = Warm.factory.apply[String, Int]()
  val entries = scala.collection.mutable.Map[String, Int]()
  entries(1) = "wrong"
}
