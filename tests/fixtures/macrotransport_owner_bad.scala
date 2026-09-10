object Main {
  val field = OwnerApi.recursive { val y = 40; y + 2 }
  def method: Int = { val local = OwnerApi.recursive { val y = 40; y + 2 }; local }
}
