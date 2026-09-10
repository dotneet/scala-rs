trait Value[A] { def value:A }; class Named extends Value[String] { def value:String="ok" }
object Main {
 def get[A](p:(String,Value[A])):A=p._2.value
 val invalid:Int=get(("key",new Named))
}
