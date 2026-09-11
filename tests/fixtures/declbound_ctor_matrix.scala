class Meter(val n:Int) extends AnyVal
class Cell[T](val value:T)
class Derived extends Cell[Meter](new Meter(7)){def read:Int=value.n;def qual:Int=this.value.n}
class LongCell extends Cell[Long](9L){def read:Long=value;def qual:Long=this.value}
class UnitCell extends Cell[Unit](()){def read:Unit=value;def qual:Unit=this.value}
object Main{def main(args:Array[String]):Unit={val d=new Derived;val b:Cell[Meter]=d;println(d.read);println(d.qual);println(b.value.n);val l=new LongCell;println(l.read);println(l.qual);val u=new UnitCell;println(u.read);println(u.qual)}}
