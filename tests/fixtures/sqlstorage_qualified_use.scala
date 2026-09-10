package sqlstoragequalified
object Main {
 def main(args: Array[String]): Unit = {
  val b = new Body
  b.count = 7
  val c = new Constructor(15, 2)
  c.count = 8
  println(b.value + ":" + b.count + ":" + c.value + ":" + c.count)
 }
}
