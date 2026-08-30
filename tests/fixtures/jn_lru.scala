// An LRU cache over `java.util.LinkedHashMap`, plus the neighbouring Java
// interop shapes: subclassing `Thread`, an anonymous `Comparator`, and
// `Arrays.sort`.
//
// Overriding `removeEldestEntry(Map.Entry[K, V])` needs the nested interface's
// own type parameters; without them the override did not match and the class
// was told it "needs to be abstract" over eight members `HashMap` and
// `AbstractMap` define — SLS 5.1.2 orders `java.util.Map` *after* `HashMap`,
// and the linearization emitted it before.
class Cache extends java.util.LinkedHashMap[String, Int](16, 0.75f, true) {
  override def removeEldestEntry(e: java.util.Map.Entry[String, Int]): Boolean =
    size() > 2
}

class Worker(n: String) extends java.lang.Thread(n) {
  override def run(): Unit = println("run " + getName)
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new Cache
    // `put` returns the *previous* value; discarded, it must not be unboxed.
    c.put("a", 1)
    c.put("b", 2)
    c.get("a")
    c.put("c", 3)
    println(c.keySet().toString)
    println(c.size())

    val t = new Worker("w1")
    t.start()
    t.join()

    // `Array("a", ...)` needs the library's varargs `apply`; build it by hand
    // so the fixture runs on the private runtime too.
    val a = new Array[String](3)
    a(0) = "pear"
    a(1) = "fig"
    a(2) = "banana"
    java.util.Arrays.sort(
      a,
      new java.util.Comparator[String] {
        override def compare(x: String, y: String): Int = x.length - y.length
      }
    )
    println(a(0) + " " + a(1) + " " + a(2))
  }
}
