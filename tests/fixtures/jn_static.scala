// JVMS 4.4.2: a method an *interface* declares — `static` ones included — must
// be named by a `CONSTANT_InterfaceMethodref`. `invokestatic` is right either
// way, only the constant's tag differs, so this type checked and then died at
// the first call with `IncompatibleClassChangeError: Method
// 'java.util.Map$Entry java.util.Map.entry(…)' must be InterfaceMethodref
// constant`. Every Java 9+ interface factory had it.
object Main {
  def main(args: Array[String]): Unit = {
    // interface statics
    val e = java.util.Map.entry("k", 7)
    println(e.getKey + "=" + e.getValue)
    val l = java.util.List.of("a", "b", "c")
    println(l.size())
    val m = java.util.Map.of("p", 1)
    println(m.size())
    println(java.util.List.copyOf(l).size())

    // interface *default* methods still go out as invokeinterface
    val it: java.util.Iterator[String] = l.iterator()
    println(it.next())
    val cs: java.lang.CharSequence = "abc"
    println(cs.length())

    // a static on a class stays a plain Methodref
    println(java.lang.Integer.valueOf(5).intValue())
    println(java.lang.String.valueOf(true))
  }
}
