package fapi
trait Owner { self =>
 def seed: Int
 class Base { def n = seed }
 trait API { type Alias = self.Base }
 object api extends API
}
object O extends Owner { def seed = 7 }
object P extends Owner { def seed = 11 }
object Holder extends Owner { def seed = 99; val other = P.api }
