trait Base[R] { def value: R }
class Outer[R](x: R) {
  def make: Base[R] = new Base[R] { self =>
    def value = x.asInstanceOf[R]
  }
}
object Main { def main(args: Array[String]): Unit = println(new Outer[String]("prefix").make.value) }
