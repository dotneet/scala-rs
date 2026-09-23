class Thunk {
  def run(fn: => Int): Int = fn
}

object Main {
  def main(args: Array[String]): Unit =
    println(ByNameMirror.inspect[Thunk])
}
