// The child comes first on the command line; its parent's evidence parameter
// only exists once the *later* file's signature pass has run. Filling the
// parent constructor in the body pass is what makes the file order irrelevant.
class Child[T: TT] extends Parent[T]

object Main {
  def main(args: Array[String]): Unit = {
    println(new Child[Int].describe)
    println(new Child[String]().describe)
  }
}
