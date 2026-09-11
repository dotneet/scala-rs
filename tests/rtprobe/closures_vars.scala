// Closures capturing vars see and make mutations; each loop iteration of a
// `for` gets a fresh binding; captured vars of every primitive type.
object Main {
  def main(args: Array[String]): Unit = {
    var x = 1
    val read = () => x
    val write = (v: Int) => x = v
    x = 5
    println(read())
    write(42)
    println(x + " " + read())

    var total = 0L
    (1 to 10).foreach(i => total += i)
    println(total)

    val fs = for (i <- 1 to 3) yield () => i * 10
    println(fs.map(_()).mkString(","))

    var j = 0
    val buf = scala.collection.mutable.ListBuffer.empty[() => Int]
    while (j < 3) { val k = j; buf += (() => k + j); j += 1 }
    println(buf.map(_()).mkString(","))

    var c: Char = 'a'; var d: Double = 1.5; var b: Boolean = false; var s: Short = 3; var by: Byte = 1; var f: Float = 0.5f
    val mut = () => { c = (c + 1).toChar; d *= 2; b = !b; s = (s * 2).toShort; by = (by + 1).toByte; f += 1 }
    mut(); mut()
    println(s"$c $d $b $s $by $f")

    def makeCounter(): (() => Int, () => Int) = { var n = 0; (() => { n += 1; n }, () => n) }
    val (inc, get) = makeCounter()
    inc(); inc(); inc()
    println(get())
    val (inc2, get2) = makeCounter()
    inc2()
    println(get2() + " " + get())

    var acc = ""
    def nested(): Unit = { def inner(s: String): Unit = acc += s; inner("a"); List("b", "c").foreach(inner) }
    nested()
    println(acc)

    var str: String = null
    val setStr = (v: String) => str = v
    setStr("set"); println(str)

    // closure capturing a var mutated after capture, in a class field
    class Holder { var v = 1; val get = () => v * 100 }
    val h = new Holder; h.v = 3
    println(h.get())
  }
}
