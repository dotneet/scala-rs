// Compiled against sam_mixin_cp_lib.scala's class files, from a directory
// and from a jar.
object Main {
  def main(a: Array[String]): Unit = {
    val k = 3
    val t: lib.OnlyVal = i => i * 2
    println(List(t.run(3), t.tag))
    val v: lib.OnlyVar = i => i + k
    v.hits += 2
    println(List(v.run(1), v.hits))
    val l: lib.OnlyLazy = i => i - 1
    println(List(l.run(1), l.l, l.l))
    val m: lib.Mixed = x => x + k
    println(List(m.base, m.count, m.tick(), m.tick(), m.label, m.label))
    println(lib.Log.lines.reverse)
  }
}
