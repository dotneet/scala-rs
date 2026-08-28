object Main {
  case class Item(name: String, qty: Int)

  def main(args: Array[String]): Unit = {
    val items = List(Item("apple", 3), Item("fig", 7), Item("pear", 3))
    println(items.map(i => i.name).mkString(","))
    println(items.filter(i => i.qty > 3).map(i => i.name).mkString(","))
    println(items.foldLeft(0)((n, i) => n + i.qty))
    println(items.sortBy(i => i.qty).map(i => i.name).mkString(","))
    println(items.maxBy(i => i.qty).name)
    println(items.minBy(i => i.qty).name)
    println(items.map(i => i.qty).distinct.length)
    println(items.exists(i => i.name == "fig"))
    println(items.find(i => i.qty == 7).map(i => i.name))

    // for-comprehension still desugars to map/flatMap/withFilter.
    val pairs = for {
      a <- List(1, 2)
      b <- List(10, 20)
      if a * b > 15
    } yield a * b
    println(pairs.mkString(","))

    // `Nil` and the empty list keep working with the new members.
    val empty: List[Int] = Nil
    println(empty.mkString("[", ",", "]"))
    println(empty.sum)
    println(empty.isEmpty)
    println(empty.map(x => x + 1).length)

    // Pattern matching over a list built with the new operators.
    val built = 1 :: (List(2) ++ List(3))
    built match {
      case List(a, b, c) => println(a + b + c)
      case _             => println("no")
    }
  }
}
