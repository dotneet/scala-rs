object Main {
  def main(args: Array[String]): Unit = {
    println("hello".partition(_ != 'l'))
    println("hello".exists(_ == 'h'))
    println("hello".exists(_ == 'z'))
    println("hello".forall(_ != 'z'))
    println("hello".forall(_ == 'h'))
    println("hello".splitAt(2))
  }
}
