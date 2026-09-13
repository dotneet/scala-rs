object Main {
  def main(args: Array[String]): Unit = {
    println(if (implicitly[Witness[Int]] != null) "ok" else "bad")
  }
}
