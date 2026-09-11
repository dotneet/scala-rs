object Main {
  var n = 0
  def main(args: Array[String]): Unit = {
    println(ByNameApi.twice({ n += 1; n }))
    println(n)
    println(ByNameApi.repeat({ n += 1; n }))
    println(n)
    println(ByNameApi.hold(() => 7)())
  }
}
