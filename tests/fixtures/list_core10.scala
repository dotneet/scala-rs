// The core `List` members that work under the private runtime (`--no-scala-library`).
// Everything here is implemented as a classfile by `crates/backend/src/runtime.rs`.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 3 :: 1 :: 4 :: 1 :: Nil
    println(xs.length)
    println(xs.size)
    println(xs.isEmpty)
    println(xs.nonEmpty)
    println(xs.head)
    println(xs.last)
    println(xs.reverse.mkString(","))
    println(xs.filter(x => x > 1).mkString(","))
    println(xs.filterNot(x => x > 1).mkString(","))
    println(xs.contains(4))
    println(xs.contains(9))
    println(xs.exists(x => x > 3))
    println(xs.exists(x => x > 9))
    println(xs.forall(x => x > 0))
    println(xs.forall(x => x > 1))
    println(xs.count(x => x == 1))
    println(xs.take(2).mkString(","))
    println(xs.drop(2).mkString(","))
    println(xs.take(0).mkString(","))
    println(xs.drop(9).mkString(","))
    println(xs.mkString)
    println(xs.mkString("[", ";", "]"))
    println(xs.map(x => x * 2).mkString(","))
    val empty: List[Int] = Nil
    println(empty.length)
    println(empty.nonEmpty)
    println(empty.mkString("<", ",", ">"))
    println(empty.reverse.mkString(","))
    println(empty.count(x => x > 0))
  }
}
