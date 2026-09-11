class Cell[T](val value:T)
class Derived extends Cell[String]("abc") {def read:Int=value.length;def qualified:Int=this.value.length}
object Main {def main(args:Array[String]):Unit={val d=new Derived;println(d.read);println(d.qualified)}}
