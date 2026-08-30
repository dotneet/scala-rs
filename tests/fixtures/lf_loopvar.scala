// A local whose *declared* class is what every frame has to say, in each shape
// that makes the merge visible: the back edge of a loop, the two arms of an
// `if` / `match` / `try` inside one, a local that changes class more than once
// per iteration, a local read after the loop at its declared type, and a loop
// nested in another loop or inside a lambda.
object Main {
  var ticks = 0

  // A `while` inside a method that returns `Unit` keeps no result on the
  // stack; the frames still have to describe `g`.
  def unitLoop(): Unit = {
    var g: Option[Int] = Some(8)
    while (g.isDefined) { g = None; ticks += 1 }
  }

  def main(args: Array[String]): Unit = {
    // `while`: Some[Int] on entry, None$ on the back edge.
    var c: Option[Int] = Some(1)
    while (c.isDefined) { c = None }
    println(c)

    // `do { } while`.
    var d: Option[Int] = Some(2)
    do { d = None } while (d.isDefined)
    println(d)

    // Nested loops, both locals changing class in the inner body.
    var o: Option[Int] = Some(3)
    var p: List[Int] = List(1, 2)
    var n = 0
    while (n < 2) {
      var m = 0
      while (m < 2) {
        o = None
        p = Nil
        m += 1
      }
      n += 1
    }
    println(o)
    println(p)

    // A generic local: List -> Nil.
    var xs: List[Int] = List(1, 2, 3)
    while (xs.nonEmpty) { xs = xs.tail }
    println(xs)

    // Several different classes in one pass through the body.
    var q: Option[String] = Some("a")
    var k = 0
    while (k < 3) {
      q = None
      q = Some("b")
      q = None
      k += 1
    }
    println(q)

    // `if` inside the loop, a different class in each arm.
    var r: Option[Int] = Some(4)
    var j = 0
    while (j < 3) {
      if (j % 2 == 0) r = None else r = Some(j)
      j += 1
    }
    println(r)

    // `match` inside the loop.
    var s: List[Int] = List(9)
    var i2 = 0
    while (i2 < 2) {
      s = s match {
        case Nil     => List(0)
        case _ :: tl => tl
      }
      i2 += 1
    }
    println(s)

    // `try` inside the loop.
    var t: Option[Int] = Some(5)
    var i3 = 0
    while (i3 < 2) {
      t = try { if (i3 == 0) None else Some(i3) } catch { case _: Throwable => None }
      i3 += 1
    }
    println(t)

    // Read after the loop, at the declared type.
    var u: Option[Int] = Some(6)
    while (u.isDefined) { u = None }
    println(u.isEmpty)

    // The desugared `for`.
    var v: Option[Int] = Some(7)
    for (x <- 1 to 3) { v = if (x == 3) Some(x) else None }
    println(v)

    // A loop inside a lambda.
    val f = (start: Int) => {
      var w: Option[Int] = Some(start)
      while (w.isDefined) { w = None }
      w.isEmpty
    }
    println(f(1))

    unitLoop()
    println(ticks)

    // A pattern binding inside the loop, writing the loop-carried local.
    var pc: Option[Int] = Some(3)
    var total = 0
    while (pc.isDefined) {
      pc match {
        case Some(x) => total += x; pc = if (x > 0) Some(x - 1) else None
        case None    => pc = None
      }
    }
    println(total)

    // A trait-typed local whose implementations are unrelated classes: the
    // declared class is an *interface*, which is a valid frame type.
    var sh: Shape = new Sq(2)
    var si = 0
    while (si < 4) {
      sh = if (si % 2 == 0) new Ci(si) else new Sq(si)
      si += 1
    }
    println(sh.area)

    // A local captured by a lambda and reassigned in the loop.
    var acc: List[Int] = Nil
    var ai = 0
    while (ai < 3) {
      val len = () => acc.length
      acc = ai :: acc
      total += len()
      ai += 1
    }
    println(acc)
    println(total)

    // `try` / `catch` / `finally` inside the loop, each writing the local, and
    // a handler that *reads* it.
    var tf: Option[String] = Some("x")
    var ti = 0
    while (ti < 3) {
      try {
        if (ti == 1) throw new RuntimeException("e")
        tf = None
      } catch {
        case _: RuntimeException => tf = Some("caught"); println(tf.isDefined)
      } finally {
        ti += 1
      }
    }
    println(tf)

    // A body whose other arm is `Nothing`.
    var nv: List[Int] = List(1, 2, 3)
    while (nv.nonEmpty) {
      nv = nv match {
        case _ :: tl => tl
        case Nil     => throw new IllegalStateException("unreachable")
      }
    }
    println(nv)
  }
}

trait Shape { def area: Int }
class Sq(val n: Int) extends Shape { def area = n * n }
class Ci(val n: Int) extends Shape { def area = 3 * n }
