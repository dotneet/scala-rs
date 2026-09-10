object Main {
  def bar: Int = 42
  def main(args: Array[String]): Unit = {
    val value = 99
    println(MacroTransport.block { def value = 40; value + 2 })
    println(MacroTransport.block { val f = (x: Int) => x + 2; if (value == 99) f(40) else 0 })
    println(MacroTransport.silent)
    println(MacroTransport.attached)
    println(MacroTransport.repeated())
    println(MacroTransport.repeated(20, 22))
    println(MacroTransport.position)
    println(MacroTransport.local)
    println(value)
    println(MacroTransport.empty)
    println(MacroTransport.empty())
    println(MacroTransport.finalEmpty(42)())
    println(MacroTransport.typeShapes)
    import macrotransportpkg._
    println(answer)
    println(macrotransportpkg.block { val local = 40; local + 2 })
  }
}
