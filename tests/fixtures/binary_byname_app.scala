object Main extends App {
  var evaluations = 0
  val result = BinaryByNameApi.evaluate {
    evaluations += 1
    42
  }
  println(s"$result:$evaluations")
}
