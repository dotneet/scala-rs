object Main {
  val b: GbTraitBase = new GbTraitBase() {
    override def greet(name: String): String = "hi " + name
  }
  def main(args: Array[String]): Unit = println(b.greet("x"))
}
