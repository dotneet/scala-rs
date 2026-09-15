trait OcValue {
  def value: String
}

class OcOuter(val prefix: String) {
  // The anonymous class reads the enclosing receiver only while its
  // constructor initializes `value`; its method then reads its own field.
  // The lambda still has to pass the hidden outer constructor argument.
  def withOuter(suffix: String): () => OcValue = () => new OcValue {
    val value: String = OcOuter.this.prefix + suffix
  }

  // A nearby control: this anonymous class does not use the enclosing
  // receiver, so its hidden outer slot may remain null as in scalac.
  def withoutOuter(): () => OcValue = () => new OcValue {
    val value: String = "constant"
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val outer = new OcOuter("outer-")
    println(outer.withOuter("capture").apply().value)
    println(outer.withoutOuter().apply().value)
  }
}
