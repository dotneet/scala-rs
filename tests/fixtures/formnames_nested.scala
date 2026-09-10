trait Value[A] { def value:A }
class Named extends Value[String] { def value:String="ok" }
class Many[A](x:A) extends Value[List[A]] { def value:List[A]=List(x) }
class Holder[+A](val value:A)
object Main {
 def get[A](p:(String,Value[A])):A=p._2.value
 def deep[A](p:Option[(String,Value[A])]):A=p.get._2.value
 def held[A](p:Holder[Value[A]]):A=p.value.value
 def callback[A](f:()=>Option[Value[A]]):A=f().get.value
 def main(args:Array[String]):Unit={
  val a:String=get(("key",new Named));println(a)
  val b:String=deep(Some(("key",new Named)));println(b)
  val c:List[Int]=get(("key",new Many(7)));println(c.mkString(","))
  val d:String=held(new Holder(new Named));println(d)
  val e:String=callback(()=>Some(new Named));println(e)
 }
}
