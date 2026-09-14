class Enclosing {
  def make(): java.io.Serializable = new java.io.Serializable {}
}

object Main {
  def main(args: Array[String]): Unit = {
    val value = new Enclosing().make()
    val bytes = new java.io.ByteArrayOutputStream()
    val stream = new java.io.ObjectOutputStream(bytes)
    stream.writeObject(value)
    stream.close()
    println("ok")
  }
}
