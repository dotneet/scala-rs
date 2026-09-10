import play.twirl.api._
object Main extends BaseScalaTemplate[Html, Format[Html]](HtmlFormat) {
 def main(args: Array[String]): Unit = {
  println(_display_(Seq[Any]("<x>", 1)).body)
  println(_display_(null).body)
  println(_display_(42).body)
 }
}
