// Compiled by **real scalac 2.13.16** against whatever compiled
// `caseabi_lib.scala`. Keys built on one side are looked up on the other, so
// the two halves have to agree on `hashCode` -- which they did not before the
// 31-fold was replaced with nsc's MurmurHash3 under `--scala-library`.

object Main {
  def main(args: Array[String]): Unit = {
    // A key built here, looked up with one built over there, and vice versa.
    val m = Map(Key(1, "a") -> "one", Key(2, "b") -> "two")
    println(m.get(Keys.make(1, "a")))
    println(m.get(Key(2, "b")))
    println(m.get(Key(3, "c")))

    val m2 = Map(Keys.make(4, "d") -> "four")
    println(m2.get(Key(4, "d")))

    val jm = new java.util.HashMap[Key, String]()
    jm.put(Keys.make(7, "z"), "seven")
    println(jm.get(Key(7, "z")))

    // The raw numbers, so a mismatch says which side moved.
    println(Key(1, "a").hashCode)
    println(Keys.make(1, "a").hashCode)
    println(Refs("p", "q").hashCode)
    println(Keys.refs("p", "q").hashCode)
    println(Tagged(5L, 1.5, true).hashCode)
    println(Keys.tagged(5L, 1.5, true).hashCode)

    // Two values that are `==` must collapse to one element in a `Set`, which
    // is a hash lookup and so needs the hashes to agree across the two halves.
    println(Set(Key(1, "a"), Keys.make(1, "a")).size)
    println(Set(Refs("p", "q"), Keys.refs("p", "q")).size)
    println(Set(Tagged(5L, 1.5, true), Keys.tagged(5L, 1.5, true)).size)
  }
}
