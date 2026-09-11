// scalac: method input_= is defined twice; the conflicting variable input
// was defined at line 4 (a var already defines its setter).
object Main {
  class C { private var input: Int = 0; def input_=(in: Int): Unit = {} }
  def main(args: Array[String]): Unit = println(new C)
}
