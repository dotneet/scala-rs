package hiddenimport {
  object Keep { def value: Int = 42 }
  object Drop { def value: Int = 1 }
}
import hiddenimport.Drop as _
import hiddenimport.Keep
object Main {
  def main(args: Array[String]): Unit = println(Keep.value)
}
