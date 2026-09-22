object Main {
  def main(args: Array[String]): Unit =
    println(new ChainProbe(0).next[Int].next[String].next[Boolean].next[Long].value)
}
