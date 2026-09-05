// An `object` called through its `apply`: now that the parameter names are
// found on the module class, a misspelled one gets nsc's own diagnostic
// (`unknown parameter name: q`) rather than "named arguments (method
// parameters not resolved)".
package p {
  package html {
    object dropdown {
      def apply(value: String = "", right: Boolean = false): String = value + right
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(p.html.dropdown(q = "x"))
  }
}
