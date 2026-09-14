object Main {
  def run(values: List[Int]): Unit = {
    values.foreach(x => java.lang.Integer.toString(x))
  }

  def main(args: Array[String]): Unit = {
    run(List(1))
    println("ok")
  }
}
