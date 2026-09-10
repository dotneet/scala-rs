object Main {
 trait Cmp[T] { def compare(a:T,b:T):Int }
 def sort[T](x:Cmp[_ >: T]):Cmp[_ >: T]=x
 def paired[T](xs:Option[T],x:Cmp[_ >: T]):Cmp[_ >: T]=x
 def paired[T](xs:Option[T]):Int=0
 def bounded[T <: java.lang.Number](x:Cmp[_ >: T]):Int=7
 def main(args:Array[String]):Unit={
  val c:Cmp[_ >: String]=sort((a:String,b:String)=>a.length-b.length)
  println(c.compare("abcd","x"))
  val p=paired(Some(""),(a:String,b:String)=>a.length-b.length)
  println(p.compare("x","abc"))
  println(bounded((a:String,b:String)=>a.length-b.length))
 }
}
