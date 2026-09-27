// The overloads read from a class file: each one has to be matched to its
// own erased descriptor (`Object`, `scala.collection.Map`, `java.util.Map`,
// `String`) before any of them can be offered.
object Client {
  def main(args: Array[String]): Unit = {
    println(hkl.Ins.forAll(List(1, 2)) { x => x + 1 })
    println(hkl.Ins.forAll(Map(1 -> "a")) { case (k, v) => k + v.length })
    val jm = new java.util.HashMap[String, Int]; jm.put("k", 1)
    println(hkl.Ins.forAll(jm) { e => e.getValue })
    println(hkl.Ins.forAll("ab") { c => c.toInt })
  }
}
