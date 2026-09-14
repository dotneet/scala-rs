import stc.SlickTreeException

object Main {
  def main(args: Array[String]): Unit = {
    val e = new SlickTreeException("msg", "detail", mark = _.startsWith("d"), removeUnmarked = false)
    println(e.mark(e.detail))
    println(e.removeUnmarked)
    println(e.parent eq null)
  }
}
