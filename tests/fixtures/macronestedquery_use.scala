object Main {
  case class Local(value: Int)
  def main(args: Array[String]): Unit = {
    var calls = 0
    println(NestedQuery.select { calls += 1; 42 })
    println(NestedQuery.select("ok"))
    println(calls)
    println(NestedQuery.refined[Local])
  }
}
