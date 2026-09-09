package multi
trait Owner { def seed: Int; class Base[A](val value: A) { def n: Int = seed }; type Alias[A] = Base[A] }
object O extends Owner { def seed = 7 }
object P extends Owner { def seed = 11 }
