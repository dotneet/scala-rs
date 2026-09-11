// scalac: forward reference to value fibs defined on line 5 extends over
// definition of value fibs (a strict local val may not refer to itself).
object Main {
  def main(args: Array[String]): Unit = {
    val fibs: LazyList[BigInt] = BigInt(0) #:: BigInt(1) #:: fibs.zip(fibs.tail).map { case (x, y) => x + y }
    println(fibs.take(5).toList)
  }
}
