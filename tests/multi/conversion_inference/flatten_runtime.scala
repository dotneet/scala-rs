object Main {
 def main(args:Array[String]):Unit={
  println(Array(Some(1),None,Some(2)).flatten.toList)
  val a:Array[Int]=Array(Some(1),None,Some(2)).flatten
  println(a.toList)
  val b:Array[String]=Array(Some("a"),None,Some("b")).flatten
  println(b.toList)
 }
}
