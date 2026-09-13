package stableprofile

trait Profile {
  trait API { type Marker }
  val api: API
}

final class Config[P <: Profile](val profile: P)

final class Concrete extends Profile {
  val api: API = new API { type Marker = Int }
}
