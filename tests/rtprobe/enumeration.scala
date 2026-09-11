// scala.Enumeration: value ids, names derived from field names by
// reflection, ordering, withName, values set, and custom Val subclasses.
object Main {
  object Color extends Enumeration { val Red, Green, Blue = Value }
  object Weekday extends Enumeration {
    type Weekday = Value
    val Mon = Value(1, "Monday"); val Tue = Value("Tuesday"); val Wed = Value
    def isWeekend(d: Weekday) = false
  }
  def main(args: Array[String]): Unit = {
    println(Color.values.toList + " " + Color.Red.id + " " + Color.Blue.id + " " + Color.maxId)
    println(Color.withName("Green") + " " + (Color.Red < Color.Blue) + " " + Color.Green.toString)
    println(Weekday.values.toList.map(v => s"${v.id}:$v"))
    println(Weekday.withName("Tuesday").id + " " + Weekday(1) + " " + Weekday.isWeekend(Weekday.Mon))
    val c: Color.Value = Color.Blue
    println(c match { case Color.Red => "r"; case Color.Blue => "b"; case _ => "?" })
    println(Color.values.map(_.id).sum + " " + Color.ValueSet(Color.Red, Color.Blue).toList)
    try Color.withName("Purple") catch { case e: NoSuchElementException => println("no Purple") }
  }
}
