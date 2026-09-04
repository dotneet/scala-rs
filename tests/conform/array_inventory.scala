// Pass an inventory table around as an Array (search, slice, zipWithIndex, fold).
case class Item(sku: String, qty: Int)

object Main {
  def restock(items: Array[Item], sku: String, add: Int): Array[Item] = {
    val i = items.indexWhere(_.sku == sku)
    if (i < 0) items :+ Item(sku, add)
    else items.updated(i, items(i).copy(qty = items(i).qty + add))
  }

  def main(args: Array[String]): Unit = {
    var stock = Array(Item("aa", 2), Item("bb", 0), Item("cc", 7))
    stock = restock(stock, "bb", 5)
    stock = restock(stock, "dd", 1)
    println(stock.map(i => s"${i.sku}:${i.qty}").mkString(" "))
    println(stock.exists(_.qty == 0))
    println(stock.count(_.qty > 1))
    println(stock.foldLeft(0)(_ + _.qty))
    println(stock.slice(1, 3).map(_.sku).mkString(","))
    println(stock.takeWhile(_.qty > 1).length)
    println(stock.zipWithIndex.map { case (it, i) => s"$i${it.sku}" }.mkString("/"))
    println(stock.map(_.sku).contains("cc"))
    println(stock.sortBy(_.qty).head.sku)
    println(stock.reverse.map(_.sku).mkString(""))
    println(stock.partition(_.qty > 1)._1.length)
  }
}
