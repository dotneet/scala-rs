package vapi
trait Owner { self =>
 def seed: Int
 class Base { def n: Int = seed }
 trait API { type Alias = self.Base }
 val api: API = new API {}
}
object O extends Owner { def seed = 19 }
