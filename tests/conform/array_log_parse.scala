// Parse log lines through an Array (split / reference array / Option / toSeq).
case class Entry(level: String, code: Int, msg: String)

object Main {
  def parse(line: String): Option[Entry] = {
    val parts: Array[String] = line.split(" ")
    if (parts.length < 3) None
    else
      parts(1).toIntOption.map(c => Entry(parts(0), c, parts.drop(2).mkString(" ")))
  }

  def summarize(entries: Array[Entry]): String =
    entries
      .groupBy(_.level)
      .toSeq
      .sortBy(_._1)
      .map { case (lv, es) => s"$lv=${es.length}" }
      .mkString(" ")

  def main(args: Array[String]): Unit = {
    val lines = Array(
      "WARN 12 disk almost full",
      "INFO 0 started",
      "ERROR 5 boom",
      "bad line",
      "INFO 1 ready"
    )
    val entries: Array[Entry] = lines.flatMap(l => parse(l))
    println(entries.length)
    println(summarize(entries))
    println(entries.map(_.code).sum)
    println(entries.filter(_.level == "INFO").map(_.msg).mkString("|"))
    println(entries.maxBy(_.code).msg)
    println(entries.toSeq.map(_.level).distinct.sorted.mkString(","))
  }
}
