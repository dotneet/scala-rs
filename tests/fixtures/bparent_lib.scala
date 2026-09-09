package bparent
trait Owner {
  def seed: Int
  class Base[A](val value: A) { def n: Int = seed }
  class Empty { def n: Int = seed }
  type EmptyAlias = Empty
  type Alias[A] = Base[A]
}
object O extends Owner { def seed = 7 }
object P extends Owner { def seed = 11 }
object Static { class Base(val n: Int) }

trait ApiOwner { self =>
  def seed: Int
  class Base { def n: Int = seed }
  trait API { type Alias = self.Base }
  object api extends API
}
object ApiP extends ApiOwner { def seed = 17 }
object Holder extends ApiOwner { def seed = 99; val other = ApiP.api }

trait ValueOwner { self =>
  def seed: Int
  class Base { def n: Int = seed }
  trait API { type Alias = self.Base }
  val api: API = new API {}
}
object ValueO extends ValueOwner { def seed = 19 }
