class Box[A](val value:A) { def choose(i:Int,j:Int):A=value }
object Main {
 implicit class Rich[A](private val b:Box[A]) { def choose[B](p:Int)(f:A=>B):B=f(b.value) }
 def main(args:Array[String]):Unit=println(new Box[String]("abc").choose(1)((s:String)=>s.length))
}
