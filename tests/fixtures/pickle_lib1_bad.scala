// Pickle completion must not invent members: a name that is in no pickle in
// List's hierarchy is still an error.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    println(xs.nosuchmember(1))
  }
}
