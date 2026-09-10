object Main {
 case class Deferred[A,B](fa:()=>A=>B) extends (A=>B) { def apply(a:A):B=fa()(a) }
 def defer[A,B](fa: => A=>B):A=>B={lazy val saved=fa;Deferred(()=>saved)}
 def main(args:Array[String]):Unit={var n=0;val f=defer({n+=1;(x:String)=>x.length+n});println(n);println(f("abc"));println(f("x"));println(n)}
}
