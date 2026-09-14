trait TestkitBoundOpsImpl[T] {
  def add(xs: Iterable[T]): Unit = ()
}

trait TestkitBoundProfile {
  trait API {
    type Ops[T] <: TestkitBoundOpsImpl[T]
    def ops[T]: Ops[T]
  }
  val api: API
}

trait TestkitBoundDb {
  type Profile <: TestkitBoundProfile
  val profile: Profile
}

abstract class TestkitBoundUser {
  val db: TestkitBoundDb
  import db.profile.api._
  val x = ops[Int]
  x.add(Seq(1))
}
