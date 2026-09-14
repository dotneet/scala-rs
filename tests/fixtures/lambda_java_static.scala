import java.lang.Integer

object Main {
  def run(values: List[Int]): Unit = {
    values.foreach(x => java.lang.Integer.toString(x))
  }

  def main(args: Array[String]): Unit = {
    run(List(1))
    // A static Java method used as a method value is lowered through a
    // synthetic class-side receiver alias. That alias must not become an
    // invokedynamic capture: Option.map still needs its receiver on stack.
    println(Option(1).map(Integer.toHexString).getOrElse("bad"))
    println(
      Option(1)
        .map(_ => Option(2).map(Integer.toHexString).getOrElse("bad"))
        .getOrElse("bad")
    )
    val shadowed = {
      // An ordinary local with the imported class's name remains a real
      // capture; only a synthetic alias at a static-selection use is skipped.
      val Integer = "shadow"
      Option(1).map(_ => Integer).getOrElse("bad")
    }
    println(shadowed)
    println("ok")
  }
}
