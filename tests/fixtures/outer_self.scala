// The slick cake shape: a component trait whose member class reaches the
// *self type*'s members. nsc types `$outer` as the self type when it is a
// subclass of the enclosing trait, and so do we — `Comp$Table.$outer` is a
// `Prof`, not a `Comp`.
// self type: nsc types `$outer` as the self type when it is a subclass.
trait Base { def profileName: String }
trait Comp { self: Prof =>
  def provider: String = self.profileName
  abstract class Table(val tableName: String) {
    def describe: String = profileName + "." + tableName + "/" + provider
  }
}
trait Prof extends Base with Comp { self: Prof => }

object Db extends Prof {
  def profileName: String = "db"
  class People extends Table("people")
}

// A local class inside a trait method still reaches the trait.
trait Meth {
  def base: Int = 5
  def run: Int = {
    class L { def v: Int = base * 2 }
    new L().v
  }
}
class MethC extends Meth

// Anonymous class inside a trait member class.
trait Outer2 {
  def k: Int = 3
  class In {
    def f: Int = {
      val r = new Runnable { def run(): Unit = () }
      r.hashCode() * 0 + k
    }
  }
}
class O2 extends Outer2

object Main {
  def main(args: Array[String]): Unit = {
    println(new Db.People().describe)
    println(new MethC().run)
    val o2 = new O2
    println(new o2.In().f)
  }
}
