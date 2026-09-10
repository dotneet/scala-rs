final class Meter(val n:Int) extends AnyVal
class Cell[T](var value:T)
class Derived[T](x:T) extends Cell[T](x)
object Main {
 var calls=0
 def main(args:Array[String]):Unit={
  val a=new Cell[String]("a");a.value="abc";println(a.value.length)
  val b=new Derived[Long](1L);b.value=9L;println(b.value)
  val c=new Cell[Array[Int]](Array(1));c.value=Array(7);println(c.value(0))
  val d=new Cell[Meter](new Meter(1));d.value=new Meter(8);println(d.value.n)
  val e=new Cell[Int](0)
  def recv:Cell[Int]={calls+=1;e}
  recv.value={calls+=10;6};println(e.value);println(calls)
  var local=1;local=2;println(local)
 }
}
