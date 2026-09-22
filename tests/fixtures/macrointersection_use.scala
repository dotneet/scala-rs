object Main {
  val value: java.io.Serializable with IntersectionMarker["label"] = IntersectionMacro.value
  def main(args: Array[String]): Unit = println(value == null)
}
