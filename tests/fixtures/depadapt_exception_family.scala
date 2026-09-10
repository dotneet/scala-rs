import scala.util.control.Exception._
object Main {
 def main(args:Array[String]):Unit={
   println(catching(classOf[NumberFormatException],classOf[IllegalArgumentException]).opt("x".toInt))
   println(catchingPromiscuously(classOf[NumberFormatException]).opt("x".toInt))
   println(catchingPromiscuously(classOf[String]).opt(4))
   ignoring(classOf[NumberFormatException]) { "x".toInt }
   println(failing(classOf[NumberFormatException]) { "x".toInt })
   println(handling(classOf[NumberFormatException]).by(_ => 9) { "x".toInt })
   println(catching({case _:NumberFormatException => 12}).apply("x".toInt))
 }
}
