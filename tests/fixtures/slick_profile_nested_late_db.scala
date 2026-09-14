import slick.jdbc.JdbcProfile

trait LateProfileProvider {
  type Profile = JdbcProfile
  val profile: Profile
}

trait LateProbeDB extends LateProfileProvider
