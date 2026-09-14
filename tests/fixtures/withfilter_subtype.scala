// A subtype may inherit a withFilter whose implementation returns a wrapper
// rather than the receiver. The static result must remain the declaration's
// base type; narrowing it to the receiver emits a failing checkcast.
trait Query {
  def withFilter(p: Int => Boolean): Query = new Query {}
  def marker: String = "query"
}

class TableQuery extends Query

object Main {
  def main(args: Array[String]): Unit = {
    val q = new TableQuery().withFilter((x: Int) => x > 0)
    println(q.marker)
  }
}
