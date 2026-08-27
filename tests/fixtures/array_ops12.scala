object Main {
  def main(args: Array[String]): Unit = {
    Array(1, 2, 3).take(2).foreach(x => println(x))
    val pf: PartialFunction[Int, String] = { case 1 => "one"; case 3 => "three" }
    Array(1, 2, 3).collect(pf).foreach(s => println(s))
    Array(1, 2).zip(List(10, 20)).foreach(t => println(t))
  }
}
