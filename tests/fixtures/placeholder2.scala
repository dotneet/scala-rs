object Main {
  def main(args: Array[String]): Unit = {
    val add: (Int, Int) => Int = _ + _
    println(add(1, 2))
    val nest: Array[Int] => Array[Int] = _.map(_ + 1)
    nest(Array(1, 2, 3)).foreach(x => println(x))
  }
}
