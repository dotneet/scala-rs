class Meter(val n:Int) extends AnyVal
class Cell[T](val value:T)
class Derived extends Cell[Meter](new Meter(7)) {def read:Int=value.n; def qualified:Int=this.value.n}
object Main {def main(args:Array[String]):Unit={val d=new Derived;println(d.read);println(d.qualified)}}
