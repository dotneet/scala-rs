class P{private def copy(s:String):Int=s.length};case class C(x:Int)extends P;object Main{def main(a:Array[String]):Unit=println(C(7).copy(4).x)}
