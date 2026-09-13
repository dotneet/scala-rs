package dbconfig

trait Profile

trait BaseDatabaseConfig {
  def forURL[P <: Profile](
      p: P,
      url: String,
      config: missing.Config,
      loader: ClassLoader
  ): Int = 1

  // A real overload: the duplicate-collapse rule must not use the unresolved
  // generic profile slot as permission to merge this `Int` overload with the
  // `Object` one above.
  def forURL[P <: Profile](
      p: P,
      url: String,
      config: Int,
      loader: ClassLoader
  ): Int = 2
}

trait DatabaseConfig extends BaseDatabaseConfig

object DatabaseConfig extends DatabaseConfig
