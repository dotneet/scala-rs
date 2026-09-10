class Cell[T](x:T){lazy val value:T=x}
object Main {val c=new Cell[Array[Int]](Array(7));val bad:String=c.value}
