trait Label {def name:String}
class Cell[T <: Label](var value:T)
object Main {val c=new Cell[String]("a");c.value="b"}
