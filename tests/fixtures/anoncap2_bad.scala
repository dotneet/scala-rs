// Only locals already in scope can be captured: a `val` defined after the
// anonymous class is not visible to it.
trait Runner { def run(): Unit }

object Main {
  def mk(): Runner = {
    val r = new Runner {
      def run(): Unit = println(later)
    }
    val later = 1
    r
  }

  def main(args: Array[String]): Unit = {
    mk().run()
  }
}
