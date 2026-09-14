trait EagerValue extends java.io.Serializable {
  def text: String
}

class EagerOuter {
  val value: String = "outer"

  // The anonymous class reads its lexical outer only while initializing
  // `copied`; its method uses the initialized field instead. scalac keeps the
  // hidden constructor argument for this shape but emits no `$outer` field.
  val valueObject: EagerValue = new EagerValue {
    val copied: String = EagerOuter.this.value
    def text: String = copied
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val original = new EagerOuter().valueObject
    val bytes = new java.io.ByteArrayOutputStream()
    val out = new java.io.ObjectOutputStream(bytes)
    out.writeObject(original)
    out.close()
    val in = new java.io.ObjectInputStream(
      new java.io.ByteArrayInputStream(bytes.toByteArray)
    )
    val restored = in.readObject().asInstanceOf[EagerValue]
    in.close()
    println(restored.text)
  }
}
