import family.Derived
trait BadClient {
  val profile: Derived
  import profile.api._
  def wrong: String = convert(4).returning
}
