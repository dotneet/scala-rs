// A nested Java generic interface has type parameters of its own:
// `interface Map<K, V> { interface Entry<K, V> { … } }`. Reading `java.util.Map`
// alone only *stubs* `Map$Entry` (its name turns up in `entrySet()`'s generic
// signature), and the stub has no type parameters — `java.util.Map.Entry[K, V]`
// was "Entry does not take type parameters".

// Implementing the nested interface from Scala.
class Pair(k: String, v: Int) extends java.util.Map.Entry[String, Int] {
  private var value = v
  def getKey(): String = k
  def getValue(): Int = value
  def setValue(nv: Int): Int = { val old = value; value = nv; old }
}

object Main {
  def show(e: java.util.Map.Entry[String, Int]): String =
    e.getKey + "=" + e.getValue

  // A wildcard application of the nested type.
  def anyKey(e: java.util.Map.Entry[_, _]): String = String.valueOf(e.getKey)

  // A nested class with *no* type parameters of its own, two levels down.
  def two(e: java.util.AbstractMap.SimpleEntry[String, Int]): String =
    e.getKey + ":" + e.getValue

  def main(args: Array[String]): Unit = {
    val m = new java.util.LinkedHashMap[String, Int]()
    m.put("a", 1)
    m.put("b", 2)
    val it = m.entrySet().iterator()
    while (it.hasNext()) {
      val e: java.util.Map.Entry[String, Int] = it.next()
      println(show(e) + " " + anyKey(e))
    }
    val p: java.util.Map.Entry[String, Int] = new Pair("z", 9)
    println(show(p))
    println(p.setValue(10))
    println(show(p))
    println(two(new java.util.AbstractMap.SimpleEntry[String, Int]("q", 4)))
  }
}
