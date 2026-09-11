// Java interop: java.util collections, conversions both ways, SAM
// conversion to Java functional interfaces, static members, boxed Java
// numbers, and Java varargs.
import scala.jdk.CollectionConverters._
import java.util.{ArrayList, HashMap => JHashMap, Collections, Arrays, Comparator}
object Main {
  def main(args: Array[String]): Unit = {
    val al = new ArrayList[String](); al.add("b"); al.add("a"); al.add("c")
    Collections.sort(al)
    println(al + " " + al.size + " " + al.get(0) + " " + al.asScala.map(_.toUpperCase) + " " + al.contains("c"))
    val jm = new JHashMap[String, Integer](); jm.put("x", 1); jm.put("y", 2)
    println(jm.get("x") + jm.get("y") + " " + jm.asScala.toList.sortBy(_._1) + " " + jm.getOrDefault("z", -1) + " " + jm.containsKey("x"))
    val back = List(3, 1, 2).asJava
    println(back + " " + back.getClass.getSimpleName.nonEmpty + " " + Map("k" -> "v").asJava.get("k"))
    val cmp: Comparator[String] = (a, b) => b.compareTo(a)
    al.sort(cmp)
    println(al)
    al.sort(Comparator.comparing[String, Integer]((s: String) => Integer.valueOf(s.charAt(0).toInt)))
    println(al)
    val r: Runnable = () => println("ran runnable")
    r.run()
    val f: java.util.function.Function[Integer, Integer] = x => x * 2
    val g: java.util.function.BiFunction[String, Integer, String] = (s, n) => s * n
    val sup: java.util.function.Supplier[String] = () => "supplied"
    val pred: java.util.function.Predicate[String] = _.isEmpty
    println(f.apply(21) + " " + g.apply("ab", 2) + " " + sup.get() + " " + pred.test("") + " " + f.andThen(f).apply(1))
    println(Arrays.asList(1, 2, 3) + " " + Arrays.toString(Array(1, 2)) + " " + java.lang.String.join(",", List("a", "b").asJava))
    println(Integer.parseInt("ff", 16) + " " + java.lang.Long.toBinaryString(5) + " " + Integer.MAX_VALUE + " " + java.lang.Double.parseDouble("1e3") + " " + Character.getNumericValue('7'))
    println(Math.floorMod(-7, 3) + " " + Math.floorDiv(-7, 3) + " " + Math.abs(-2.5) + " " + Math.pow(2, 10) + " " + Math.hypot(3, 4) + " " + StrictMath.sqrt(2.0))
    val boxed: java.lang.Integer = 42; val unboxed: Int = boxed; val sum = boxed + 1
    println(unboxed + " " + sum + " " + boxed.compareTo(41) + " " + boxed.doubleValue)
    val it = al.iterator(); var acc = ""
    while (it.hasNext) acc += it.next()
    println(acc)
    val jl = new java.util.LinkedList[Int](); jl.add(3); jl.addFirst(1)
    println(jl + " " + jl.getFirst + " " + jl.peekLast)
    val sb = new java.lang.StringBuilder("x"); sb.append(1).append('c').append(2.5).insert(0, true)
    println(sb)
    val opt = java.util.Optional.of("present")
    println(opt.map[String](_.toUpperCase).orElse("none"))
    val stream = java.util.stream.IntStream.rangeClosed(1, 5).map(x => x * x).sum()
    println(stream)
    println(String.format("%s|%5.2f|%03d", "fmt", Double.box(3.14159), Int.box(7)))
    val ts = new java.util.TreeSet[String](List("z", "m", "a").asJava)
    println(ts + " " + ts.first + " " + ts.headSet("n"))
    println(Objects.equals(null, null) + " " + java.util.Objects.hash(Int.box(1), "a"))
  }
  object Objects { def equals(a: AnyRef, b: AnyRef): Boolean = java.util.Objects.equals(a, b) }
}
