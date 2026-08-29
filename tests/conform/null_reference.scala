object Main {
  var handler: (String) => Unit = null
  var pair: (Int, String) = null
  def main(args: Array[String]): Unit = {
    println(handler == null)
    println(pair == null)
    handler = s => println("got " + s)
    handler("x")
    pair = (1, "a")
    println(pair)
  }
}
