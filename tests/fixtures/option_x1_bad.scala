object Main {
  def main(args: Array[String]): Unit = {
    val s: Option[Int] = Some(3)
    println(s.noSuchOptionMember)
  }
}
