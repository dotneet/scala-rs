object Main { def main(args:Array[String]):Unit=println(scala.util.control.Exception.catching(classOf[NumberFormatException]).opt("x".toInt)) }
