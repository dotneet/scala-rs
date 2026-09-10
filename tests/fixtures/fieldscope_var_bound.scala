trait Label {def name:String}
class L(val name:String) extends Label
class Cell[T <: Label](var value:T)
object Main {def main(args:Array[String]):Unit={val c=new Cell[L](new L("a"));c.value=new L("b");println(c.value.name)}}
