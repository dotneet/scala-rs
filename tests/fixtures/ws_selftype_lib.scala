// The library half of the `ws_selftype` pair: the class a self type names.
//
// It has to arrive as *class files*, because that is the whole point of the
// pair -- a `-cp` class's member list is empty until something asks for a
// name, so a self type that names one offered nothing to the template's
// scope. Written in source, `Table` has all its members in the symbol table
// before any self type is bound, and the bug does not appear.
package wsl

class Table[T](val tableName: String) {
  def column[C](n: String): String = tableName + "." + n
  def describe: String = "table " + tableName
}

/** A second class, for the compound self type `A with B`. */
trait Tagged {
  def tag: String = "tagged"
}
