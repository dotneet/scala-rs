package apiouter
trait Owner { self =>
 class Base(val n: Int)
 trait API { type Alias = self.Base }
 object api extends API
}
object O extends Owner
