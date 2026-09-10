import typeidentitymacro._
object Main {
  def main(args: Array[String]): Unit = {
    println(API.names[String, Int])
    println(new Owned[String].names[Int])
    println(API.value)
    println(API.sum(20)(22))
    println(implicitly[Evidence[String]].value)
  }
}
