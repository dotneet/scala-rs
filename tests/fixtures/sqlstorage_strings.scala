object Main {
 def decoded(a: Array[Byte]): String = new String(a)
 def main(args: Array[String]): Unit = {
  val bytes = Array[Byte](65,66,67)
  val chars = Array[Char]('x','y','z')
  println(decoded(bytes))
  println(new java.lang.String(bytes, 1, 2))
  println(new String(bytes, "UTF-8"))
  println(new String(bytes, java.nio.charset.StandardCharsets.UTF_8))
  println(new String(bytes, 1, 2, "UTF-8"))
  println(new String(bytes, 1, 2, java.nio.charset.StandardCharsets.UTF_8))
  println(new String(chars))
  println(new String(chars, 1, 2))
  println(new String("copy"))
  println(new String(new java.lang.StringBuilder("builder")))
  println(new String(new java.lang.StringBuffer("buffer")))
  type Text = java.lang.String
  val text: Text = new Text(bytes)
  println(text)
  println(new String().length)
  val list: List[String] = List(new String(bytes), "literal")
  println(list.mkString("/"));
  { class String(val value: Int); println(new String(7).value) }
 }
}
