package scala.extensionsample {
  trait Syntax {
    implicit class TextOps(private val text: String) {
      def bracketed: String = "[" + text + "]"
    }
  }
}
package scala {
  package object extensionsample extends extensionsample.Syntax
}
