import Replacement._
class Answer
object Replacement { implicit val answer:Int=2 }
object Main {
 def main(args:Array[String]):Unit=println(implicitly[Int])
}
