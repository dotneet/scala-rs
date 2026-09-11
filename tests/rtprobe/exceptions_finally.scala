// try/catch/finally ordering, try as an expression, return from inside
// finally and catch, nested handlers, rethrow, and exceptions from catch.
object Main {
  val log = new StringBuilder
  def l(s: String): Unit = log.append(s).append(' ')
  def dump(): String = { val s = log.toString.trim; log.clear(); s }

  def f1(): Int = try { l("try"); 1 } finally { l("finally") }
  def f2(): Int = try { l("try"); throw new RuntimeException("x") } catch { case _: RuntimeException => l("catch"); 2 } finally { l("finally") }
  def f3(): Int = { try { return 3 } finally { l("finally-after-return") } }
  @annotation.nowarn def f4(): Int = { try { return 4 } finally { return 5 } }
  def f5(): String = try { try throw new IllegalArgumentException("inner") finally l("inner-finally") } catch { case e: IllegalArgumentException => l("outer-catch"); e.getMessage }
  def f6(): String = try { try throw new RuntimeException("a") catch { case e: RuntimeException => throw new IllegalStateException("from-catch:" + e.getMessage) } } catch { case e: IllegalStateException => e.getMessage }
  def f7(x: Int): String = try {
    if (x == 0) throw new ArithmeticException("zero")
    if (x < 0) throw new IllegalArgumentException("neg")
    "ok" + (10 / x)
  } catch {
    case e: ArithmeticException if e.getMessage == "zero" => "arith-zero"
    case e @ (_: IllegalArgumentException | _: NullPointerException) => "iae-or-npe:" + e.getClass.getSimpleName
  }
  def f8(): Int = { var i = 0; try { i = 1; i } finally { i = 100 } }
  def f9(): Int = {
    var i = 0
    while (i < 10) { try { if (i == 3) return i * 10 } finally { l("f" + i) }; i += 1 }
    -1
  }
  class MyEx(val code: Int) extends Exception("code " + code)
  def f10(): String = try throw new MyEx(42) catch { case e: MyEx => s"${e.code} ${e.getMessage}" }
  def f11(): String = {
    val r = try { Integer.parseInt("zz") } catch { case _: NumberFormatException => -1 }
    "parsed " + r
  }
  def f12(): Unit = try { l("unit-try") } finally l("unit-finally")
  def f13(): String = try { null.asInstanceOf[String].length.toString } catch { case _: NullPointerException => "npe" }
  def f14(): String = try { throw new Error("err") } catch { case t: Throwable => "throwable " + t.getMessage }
  def f15(): Long = try 5L finally l("long-finally")
  def f16(): Double = { val d = try { 1.5 } catch { case _: Exception => 0.0 }; d * 2 }

  def main(args: Array[String]): Unit = {
    println(f1() + " " + dump()); println(f2() + " " + dump()); println(f3() + " " + dump()); println(f4())
    println(f5() + " " + dump()); println(f6()); List(2, 0, -1).foreach(x => println(f7(x)))
    println(f8()); println(f9() + " " + dump()); println(f10()); println(f11()); f12(); println(dump())
    println(f13()); println(f14()); println(f15() + " " + dump()); println(f16())
    try { f7(Int.MinValue); println("no throw") } catch { case e: Exception => println("unexpected " + e) }
    val caught = try { List(1, 2, 3)(5); "none" } catch { case e: IndexOutOfBoundsException => "ioobe" }
    println(caught)
  }
}
