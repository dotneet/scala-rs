import multi.P._
class C extends Alias[Int](5)
object Main { def main(args:Array[String]):Unit = println(new C().n) }