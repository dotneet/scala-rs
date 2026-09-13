package resultprefix

class Factory {
  def forURL(s: String): Int = 1
}

trait BasicBackend {
  type DatabaseFactory
  val Database: DatabaseFactory
}

trait JdbcBackend extends BasicBackend {
  type DatabaseFactory = Factory
}

trait BasicProfile {
  val backend: BasicBackend

  trait API {
    val Database: backend.DatabaseFactory = backend.Database
  }

  val api: API
}

trait JdbcProfile extends BasicProfile {
  override val backend: JdbcBackend

  trait API extends super.API

  override val api: API
}

trait BlockingProfile extends JdbcProfile {
  trait BlockingAPI extends API
  val blockingApi: BlockingAPI
}

object Driver extends BlockingProfile {
  val backend: JdbcBackend = new JdbcBackend {
    val Database: DatabaseFactory = new Factory
  }
  val api: API = new API {}
  val blockingApi: BlockingAPI = new BlockingAPI {}
}
