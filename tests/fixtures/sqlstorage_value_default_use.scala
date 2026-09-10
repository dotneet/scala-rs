object Main {
 def main(args: Array[String]): Unit = {
  println(new ValueDefaults.Slot[String]().describe)
  println(ValueDefaults.default[Int].describe)
  println(new ValueDefaults.Slot[Int](false).describe)
 }
}
