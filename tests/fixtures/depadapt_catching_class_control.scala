import scala.util.control.Exception.catching; object Main { def main(args:Array[String]):Unit=println(catching(classOf[String]).opt(1)) }
