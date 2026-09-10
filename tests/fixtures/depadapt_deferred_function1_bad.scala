object Main {
 case class Deferred[A,B](fa:()=>A=>B) extends (A=>B) { def apply(a:A):B=fa()(a) }
 val bad: String=>String=Deferred(()=>(x:String)=>x.length)
}
