// Escapes in triple-quoted strings and interpolations, checked against
// scalac 2.13.16 (crates/cli/tests/rto.rs). Unicode escapes in triple-quoted
// literals and `raw` interpolations are deprecated but still processed.
object Main {
  def show(s: String) = s.map(c => if (c < ' ') f"<${c.toInt}%02x>" else c.toString).mkString
  def main(args: Array[String]): Unit = {
    val x = 1
    println(show(s"tab\u0009tab"))
    println(show(s"""tab\u0009tab"""))
    println(show(s"""tab\ttab"""))
    println(show(s"a\u0009$x"))
    println(show(raw"tab\u0009tab"))
    println(show(raw"""tab\u0009tab"""))
    println(show(raw"a\nb"))
    println(show("""tab\u0009tab"""))
    println(show("""no\ttab"""))
    println(show(f"""t\u0009$x%d \t"""))
    println(show(s"\\u0040"))
    println(show(raw"\\u0040"))
    println(show(s"""a\u000d\u000ab"""))
    println(show(s"""q\"x"""))
    println(show(s"a$"b"))
    println(show(s"""a\""""))
    println(show(s"""x\\y\\\\z"""))
  }
}
