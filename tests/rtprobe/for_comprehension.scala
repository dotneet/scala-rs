// for comprehensions: multiple generators, guards, value definitions,
// refutable patterns (which filter), for over Option/Map/Range/String, and
// the side-effect order of generators and guards.
object Main {
  def main(args: Array[String]): Unit = {
    println(for (i <- 1 to 3; j <- 1 to i) yield (i, j))
    println(for (i <- 1 to 10 if i % 2 == 0; if i > 4) yield i)
    println(for { i <- List(1, 2, 3); sq = i * i; if sq > 1; s = sq.toString } yield s + "!")
    val pairs: List[Any] = List((1, "a"), "notpair", (2, "b"), (3, 4))
    println(for ((n: Int, s: String) <- pairs) yield s * n)
    val opts = List(Some(1), None, Some(3))
    println(for (Some(v) <- opts) yield v)
    println(for { a <- Some(2); b <- Some(3) } yield a * b)
    println(for { a <- Some(2); b <- None: Option[Int] } yield a * b)
    val m = Map("x" -> 1, "y" -> 2)
    println((for ((k, v) <- m) yield (v, k)).toList.sorted)
    println(for (c <- "abc") yield c.toUpper)
    val log = new StringBuilder
    val r = for { i <- { log.append("g1;"); List(1, 2) }; if { log.append(s"f$i;"); i > 1 }; j <- { log.append(s"g2($i);"); List(10, 20) } } yield i * j
    println(r + " " + log)
    var sum = 0
    for (i <- 1 to 4; j <- 1 to 2) sum += i * j
    println(sum)
    for ((a, b) <- List((1, 2), (3, 4))) print(a + b + " ")
    println()
    println(for (x <- Vector(1, 2); y <- List("a")) yield (x, y))
    println(for (x <- Set(1, 2, 3); y = x % 2) yield y)
    println(for (i <- 0 until 10 by 3) yield i)
    println(for (i <- 10 to 1 by -4) yield i)
    println((for (i <- 1 to 3) yield i.toDouble / 2).sum)
    val nested = for { xs <- List(List(1, 2), List(3)); x <- xs } yield x * 10
    println(nested)
    val it = for (i <- Iterator(1, 2, 3) if i != 2) yield i
    println(it.toList)
    println(for (i <- List(1, 2, 3); (a, b) = (i, i * 2)) yield a + b)
    println(for (a :: _ <- List(List(1, 2), Nil, List(5))) yield a)
  }
}
