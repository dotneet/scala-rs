import stableprofile.{Config, Concrete}

object StableProfileUse {
  val c: Config[Concrete] = null
  import c.profile.api._
  val marker: Marker = null.asInstanceOf[Marker]
  val stable: c.profile.type = null.asInstanceOf[c.profile.type]
}
