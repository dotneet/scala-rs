object Main {
  def first[T](a: Array[T]) = a.head
  def firstMap[T: scala.reflect.ClassTag](a: Array[T]) = a.map(x => x)
  def main(args: Array[String]): Unit = {
    println(first(Array(1, 2, 3)))
    println(first(Array("a", "b")))
    val ar: Array[AnyRef] = Array("x", "y")
    println(ar.head)
    firstMap(Array(10, 20)).foreach(x => println(x))
  }
}
