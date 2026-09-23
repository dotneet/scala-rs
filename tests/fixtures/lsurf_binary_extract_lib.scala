package binaryextractor

final class Extracted(val get: String) extends AnyVal {
  def isEmpty: Boolean = get.isEmpty
}

object Word {
  def unapply(value: String): Extracted =
    new Extracted(if (value.startsWith("x")) value else "")
}
