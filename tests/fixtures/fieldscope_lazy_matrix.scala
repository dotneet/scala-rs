final class Meter(val n:Int) extends AnyVal
trait Label {def name:String}
class L(val name:String) extends Label
class Cell[T](init:()=>T) {lazy val value:T=init()}
object Main {
 var calls=0
 def main(args:Array[String]):Unit={
  val a=new Cell[Array[Int]](()=>{calls+=1;Array(7)})
  println(a.value(0));println(a.value(0));println(calls)
  val b=new Cell[Label](()=>new L("b"));println(b.value.name)
  val c=new Cell[Meter](()=>new Meter(8));println(c.value.n)
  val d=new Cell[Unit](()=>{calls+=1;()});println(d.value);println(d.value);println(calls)
 }
}
