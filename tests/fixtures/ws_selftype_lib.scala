// The library half of the `ws_selftype` pair: the class a self type names,
// reached from another compilation unit through `-cp`.
package wsl

class Table[T](val tableName: String) {
  def column[C](n: String): String = tableName + "." + n
  def describe: String = "table " + tableName
}

trait Tagged {
  def tag: String = "tagged"
}
