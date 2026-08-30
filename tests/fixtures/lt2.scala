// Local traits stacked on each other: linearization, `override`,
// `abstract override` and `super` all have to run through the same
// `T$class` statics a top-level trait uses.
trait TopTrait { def top = "top" }

object Main {
  def stacking(): Unit = {
    trait A { def name = "A" }
    trait B extends A { override def name = "B(" + super.name + ")" }
    trait C extends A { override def name = "C(" + super.name + ")" }
    class ABC extends B with C
    class ACB extends C with B
    println(new ABC().name)
    println(new ACB().name)
  }

  def abstractOverride(): Unit = {
    trait Base { def f: String = "base" }
    trait Mid extends Base { abstract override def f: String = "mid(" + super.f + ")" }
    trait Late extends Base { override def f: String = "late" }
    class K extends Late with Mid
    println(new K().f)
  }

  def localTraitExtendsLocalTrait(): Unit = {
    trait Inner { def a = "a" }
    trait Outer extends Inner { def b = a + "b" }
    class Both extends Outer
    println(new Both().b)
    val o: Outer = new Both
    println(o.a)
  }

  def overriddenMember(): Unit = {
    trait T { def m = "T.m"; val label = "T.label" }
    class Over extends T { override def m = "Over.m/" + super.m }
    println(new Over().m)
    println(new Over().label)
  }

  def extendsTopLevelTrait(): Unit = {
    trait L extends TopTrait { def both = top + "/L" }
    class LC extends L
    println(new LC().both)
    println((new LC(): TopTrait).top)
  }

  def genericLocalTrait(): Unit = {
    trait Box[A] { def get: A; def show = "box:" + get }
    class IntBox extends Box[Int] { def get = 7 }
    println(new IntBox().show)
    val b: Box[Int] = new IntBox
    println(b.get)
  }

  def selfTyped(): Unit = {
    trait Named { def name: String }
    trait Greeter { self: Named => def greet = "hi " + name }
    class G extends Named with Greeter { def name = "g" }
    println(new G().greet)
  }

  def main(args: Array[String]): Unit = {
    stacking()
    abstractOverride()
    localTraitExtendsLocalTrait()
    overriddenMember()
    extendsTopLevelTrait()
    genericLocalTrait()
    selfTyped()
  }
}
