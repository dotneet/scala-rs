import slick.jdbc.JdbcProfile

trait SlickAbstractProfileDb {
  type Profile <: JdbcProfile
  val profile: Profile
}
