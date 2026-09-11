object Main {
 def main(args:Array[String]):Unit={
  println(Seq(Array("a","b"),Array("c")).flatten.mkString(","))
  println(Seq(Array(true,false),Array(true)).flatten.mkString(","))
  println(Seq(Array(1L,2L),Array(3L)).flatten.mkString(","))
  println(Seq(Array((),()),Array(())).flatten.length)
 }
}
