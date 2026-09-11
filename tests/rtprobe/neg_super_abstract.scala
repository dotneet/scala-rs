// scalac: method f in trait A is accessed from super. It may not be
// abstract unless it is overridden by a member declared `abstract' and
// `override'.
object Main {
  trait A { def f: Int }
  trait B extends A { override def f: Int = super.f + 1 }
  def main(args: Array[String]): Unit = println(new B { }.f)
}
