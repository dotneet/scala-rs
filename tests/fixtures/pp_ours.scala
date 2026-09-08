// Compiled by scala-rs against `pp_nsc_lib`'s scalac-produced class files.
// Scala 2.13 lets an empty parameter list be auto-applied, so both `f()` and
// `f` have to reach the same method.
object Main {
  def main(args: Array[String]): Unit = {
    println(NsKeys.tick())
    println(NsKeys.tick)
    println(NsKeys.label)
    NsKeys.unit()
    println(NsKeys.withImplicit())
    println(NsKeys.curried(3)(4))
    println(NsEmpty())
    println(NsEmpty.apply())
    println(NsEmpty().copy())
    val t: NsTicker = NsKeys
    println(t.tick())
  }
}
