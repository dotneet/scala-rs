// `scala.Enumeration`.
//
// Almost everything here is read out of `scala/Enumeration.class`'s own
// `ScalaSignature`: `values`, `withName`, `apply`, `maxId`, and the whole
// `ValueSet` surface. What made that possible is that completion now also asks
// a user class's *library ancestors* -- `object Color extends Enumeration` is
// not itself a `scala.*` class, so before this it inherited only what the
// prelude had hand-written (`Value` and `Value.id`, and nothing else).
//
// The ids come from the library at run time: `Value()` reads and bumps
// `Enumeration.nextId`, so `val Red, Green, Blue = Value` numbers them 0, 1, 2
// simply by evaluating the right-hand side once per name.
//
// Real scala-library only -- the private runtime has no `scala/Enumeration`.

object Color extends Enumeration {
  val Red, Green, Blue = Value
  val Custom = Value(10, "custom")
}

object Weekday extends Enumeration {
  type Weekday = Value
  val Mon = Value(1)
  val Tue = Value("tuesday")
  val Wed = Value
}

object Main {
  def describe(c: Color.Value): String = c match {
    case Color.Red    => "red"
    case Color.Green  => "green"
    case Color.Custom => "custom!"
    case other        => "other:" + other
  }

  def main(args: Array[String]): Unit = {
    // the multiple assignment numbers them in order
    println((Color.Red, Color.Red.id, Color.Custom.id))
    println((Color.Green.id, Color.Blue.id))
    println(Color.values.toList)
    println(Color.withName("Green") == Color.Green)
    println(Color.Blue)
    println(Color.values.filter(_.id < 2))
    println(Color.Custom.toString)
    println(Color.maxId)
    println(Color(1))
    println(Color.values.size)
    println(Color.values.contains(Color.Red))
    // stable-identifier patterns
    println(describe(Color.Red))
    println(describe(Color.Custom))
    println(describe(Color.Blue))
    // the other two `Value` overloads, and a `type` alias to `Value`
    println((Weekday.Mon.id, Weekday.Tue.id, Weekday.Wed.id))
    println(Weekday.values.toList)
    println(Weekday.Tue.toString)
    val d: Weekday.Weekday = Weekday.Mon
    println(d)
    // `Value` is `Ordered`, so a `ValueSet` is sorted and comparable
    println(Color.Red < Color.Blue)
    println(Color.values.toList.map(_.id))
    // `withName` on a name that is not there is an exception, not a wrong value
    println(
      try { Weekday.withName("nope").toString }
      catch { case e: NoSuchElementException => "no such: " + e.getMessage }
    )
  }
}
