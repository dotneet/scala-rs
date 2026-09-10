class Cell[T](x:T){lazy val value:T=x};object Main {def main(args:Array[String]):Unit={val c=new Cell[String]("value");println(c.value.length)}}
