object Main {
  implicit val n: Int = 41
  def main(args: Array[String]): Unit = {
    println(1 + "x")
    println(implicitly[Int])
    println(identity(42))
    locally {
      println("here")
    }
  }
}
