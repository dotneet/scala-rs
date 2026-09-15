trait OcBadValue {
  def value: String
}

class OcBadOuter {
  def broken(): () => OcBadValue = () => new OcBadValue {
    val value: String = missingLocal
  }
}
