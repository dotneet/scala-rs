object Main {
  val field = OwnerApi.wrap { val y = 40; y + 2 }
  def method: Int = { val local = OwnerApi.wrap { val y = 40; y + 2 }; local }
  val definitions = OwnerApi.wrap { def value(): Int = 40; val y = value(); y + 2 }
  def main(args: Array[String]): Unit = { println(field); println(method); println(definitions) }
}
