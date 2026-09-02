// An unknown field name in `.copy(...)` on a class only reached by another
// file's inheritance chain (`t5_case_copy_qual.scala`) is still an error,
// through the same `named_arg_slots` path an ordinary `.copy()` already
// used.
package t5cpc {
  final case class Item(name: String, tag: Int = 0)
}

package t5cpd {
  object Helper {
    def bad(i: t5cpc.Item) = i.copy(nope = 1)
  }
}

object Main {
  def main(args: Array[String]): Unit = println(t5cpd.Helper.bad(t5cpc.Item("x")))
}
