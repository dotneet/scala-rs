class Cell[T](x:T){lazy val value:T=x};object Main {def main(args:Array[String]):Unit={val c=new Cell[Array[Int]](Array(7));println(c.value(0))}}
