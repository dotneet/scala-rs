// scala-rs rejects: a stable path through a method (`Date.unanchored`) as
// an extractor.
object Main {
  val Date = """(\d{4})-(\d{2})-(\d{2})""".r
  def main(args: Array[String]): Unit = {
    "on 2024-03-15 ok" match { case Date.unanchored(y, _, _) => println("unanchored " + y); case _ => println("no") }
  }
}
