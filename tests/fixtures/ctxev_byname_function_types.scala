object Main {
  var count = 0
  def twice(value: => Int): Int = value + value
  def render(value: => Int): String = value.toString
  def f: ((=> Int) => Int) = value => twice(value)
  def nested: (String => (=> Int) => String) = prefix => value => prefix + render(value)
  def functions: ((=> (() => Int)) => Int) = value => value() + value()
  def main(args: Array[String]): Unit = {
    println(f({ count += 1; count }))
    println(count)
    println(nested("n=")({ count += 1; count }))
    println(count)
    println(functions(() => { count += 1; count }))
    println(count)
  }
}
