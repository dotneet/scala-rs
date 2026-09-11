class Allowed extends importedcopy.Restricted { def call: String = inherited("protected") }
object Main {
  def main(args: Array[String]): Unit = {
    println(importedcopy.Ordinary(1).copy().value)
    println(importedcopy.Ordinary(1).copy(value = 7).value)
    println(importedcopy.Custom(1).copy("custom"))
    println(importedcopy.Inherited(1).copy("inherited"))
    println(new Allowed().call)
  }
}
