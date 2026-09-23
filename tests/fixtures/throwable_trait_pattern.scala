trait Compatible {
  def value: Int
}

class Mixed extends RuntimeException with Compatible {
  def value: Int = 7
}

object Main {
  def main(args: Array[String]): Unit = {
    val error: Throwable = new Mixed
    val result = error match {
      case compatible: Compatible => compatible.value
      case _ => 0
    }
    println(result)
  }
}
