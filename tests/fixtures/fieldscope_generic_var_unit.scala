class Cell[T](var value:T);object Main {def main(args:Array[String]):Unit={val c=new Cell[Unit](());c.value=();println(c.value)}}
