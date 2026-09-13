object Main {
  def classify(error: Throwable): String = error match {
    case _: AbstractMethodError => "abstract"
    case _ => "other"
  }

  def main(args: Array[String]): Unit = {
    println(classify(PatternErrorHolder.identity(new AbstractMethodError("missing"))))
  }
}
