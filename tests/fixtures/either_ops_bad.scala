object Main {
  def main(args: Array[String]): Unit = {
    val r: Either[String, Int] = Right(1)
    println(r.noSuchEitherMember)
  }
}
