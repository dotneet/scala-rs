// Each block's locals go out of scope when it ends, and their slots are
// reused by what follows, as javac does.
object Slots {
  def many(x: Int): Long = {
    var acc = 0L
    acc += { val a0 = x + 0; val b0 = a0 * 2L; b0 }
    acc += { val a1 = x + 1; val b1 = a1 * 2L; b1 }
    acc += { val a2 = x + 2; val b2 = a2 * 2L; b2 }
    acc += { val a3 = x + 3; val b3 = a3 * 2L; b3 }
    acc += { val a4 = x + 4; val b4 = a4 * 2L; b4 }
    acc += { val a5 = x + 5; val b5 = a5 * 2L; b5 }
    acc += { val a6 = x + 6; val b6 = a6 * 2L; b6 }
    acc += { val a7 = x + 7; val b7 = a7 * 2L; b7 }
    acc += { val a8 = x + 8; val b8 = a8 * 2L; b8 }
    acc += { val a9 = x + 9; val b9 = a9 * 2L; b9 }
    acc += { val a10 = x + 10; val b10 = a10 * 2L; b10 }
    acc += { val a11 = x + 11; val b11 = a11 * 2L; b11 }
    acc += { val a12 = x + 12; val b12 = a12 * 2L; b12 }
    acc += { val a13 = x + 13; val b13 = a13 * 2L; b13 }
    acc += { val a14 = x + 14; val b14 = a14 * 2L; b14 }
    acc += { val a15 = x + 15; val b15 = a15 * 2L; b15 }
    acc += { val a16 = x + 16; val b16 = a16 * 2L; b16 }
    acc += { val a17 = x + 17; val b17 = a17 * 2L; b17 }
    acc += { val a18 = x + 18; val b18 = a18 * 2L; b18 }
    acc += { val a19 = x + 19; val b19 = a19 * 2L; b19 }
    acc += { val a20 = x + 20; val b20 = a20 * 2L; b20 }
    acc += { val a21 = x + 21; val b21 = a21 * 2L; b21 }
    acc += { val a22 = x + 22; val b22 = a22 * 2L; b22 }
    acc += { val a23 = x + 23; val b23 = a23 * 2L; b23 }
    acc += { val a24 = x + 24; val b24 = a24 * 2L; b24 }
    acc += { val a25 = x + 25; val b25 = a25 * 2L; b25 }
    acc += { val a26 = x + 26; val b26 = a26 * 2L; b26 }
    acc += { val a27 = x + 27; val b27 = a27 * 2L; b27 }
    acc += { val a28 = x + 28; val b28 = a28 * 2L; b28 }
    acc += { val a29 = x + 29; val b29 = a29 * 2L; b29 }
    acc += { val a30 = x + 30; val b30 = a30 * 2L; b30 }
    acc += { val a31 = x + 31; val b31 = a31 * 2L; b31 }
    acc += { val a32 = x + 32; val b32 = a32 * 2L; b32 }
    acc += { val a33 = x + 33; val b33 = a33 * 2L; b33 }
    acc += { val a34 = x + 34; val b34 = a34 * 2L; b34 }
    acc += { val a35 = x + 35; val b35 = a35 * 2L; b35 }
    acc += { val a36 = x + 36; val b36 = a36 * 2L; b36 }
    acc += { val a37 = x + 37; val b37 = a37 * 2L; b37 }
    acc += { val a38 = x + 38; val b38 = a38 * 2L; b38 }
    acc += { val a39 = x + 39; val b39 = a39 * 2L; b39 }
    acc
  }
  def nested(x: Int): Int =
    if (x > 1) { val y = x - 1; if (y > 2) { val z = y * 2; z } else { val w = y + 3L; w.toInt } }
    else { val v = x.toDouble; v.toInt }
  // The loop head's frame must not claim the released String slot, which
  // the loop body reuses for an Int.
  def loopy(n: Int): Int = {
    var acc = 0
    acc += { val s = "ab" * n; s.length }
    while (acc < 100) { acc += { val d = acc * 2 + 1; d } }
    acc
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Slots.many(3))
    println((0 to 6).map(Slots.nested).mkString(","))
    println(Slots.loopy(3))
  }
}
