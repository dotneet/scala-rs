// A fixture collecting 7 roots of implicit-not-found / member-not-visible.
//
// 1. An applied abstract type member does not conform to its own upper bound (`CT[U] <: TT[U]`)
// 2. A context bound's evidence type is not expanded through the self type
// 3. Reading a companion object's `protected` member from the companion side
// 4. A nested `private[pkg] object`
// 5. An anonymous class's self alias (`new T { base => … }`)
// 6. A non-stable `def` shadowing the extractor in a constructor pattern's function position
// 7. A Java `Object` return type becoming `Any`, so `eq`/`ne` are missing
// 8. `scala.collection.Map`'s `get`/`contains`/`getOrElse`/`apply`

package implfind {

  // ---- 1 / 2: cake abstract type members and context bounds ---------------
  trait TT[T] { def name: String }
  trait BB[T] extends TT[T]

  trait Comp { self: Prof =>
    type CT[T] <: TT[T]
    type BCT[T] <: CT[T] with BB[T]
  }

  trait Prof extends Comp { self: Prof =>
    // Search through the upper bound: the only candidate is the `BCT[U]` evidence.
    def viaBound[U: BCT](u: U): String = implicitly[TT[U]].name
    // The requesting side, under the same name. It becomes the alias the self type made concrete.
    def viaSelf[U: BCT](u: U): String = implicitly[BCT[U]].name
  }

  // A component whose self type names a concrete profile. The evidence of the
  // `[U : BCT]` written here has to become `JT[U] with BB[U]` through the self
  // type (the body's `implicitly[BCT[U]]` does).
  trait JComp extends Comp { self: JProf =>
    def viaComponent[U: BCT](u: U): String = implicitly[BCT[U]].name
    def viaComponentJT[U: BCT](u: U): String = implicitly[JT[U]].name
  }

  trait JProf extends Prof with JComp {
    type CT[T] = JT[T]
    type BCT[T] = JT[T] with BB[T]
  }

  trait JT[T] extends TT[T]

  object Cake extends JProf

  // ---- 3: protected in a companion object ---------------------------------
  trait Prot {
    def viaTrait: Int = Prot.hidden
  }
  object Prot {
    protected val hidden: Int = 7
  }

  class ProtC {
    def viaClass: Int = ProtC.hidden
  }
  object ProtC {
    protected val hidden: Int = 11
  }

  // ---- 4: a nested private[pkg] object ------------------------------------
  object Outer {
    private[implfind] object Inner { val v: Int = 13 }
    private[implfind] class InnerC { val v: Int = 17 }
  }

  class UsesInner {
    def a: Int = Outer.Inner.v
    def b: Int = new Outer.InnerC().v
  }

  // ---- 5: an anonymous class's self alias ---------------------------------
  trait Tag {
    def label: String
    def tagged(i: Int): Tag
  }

  object Anon {
    def run: String = {
      val outer = new Tag { base =>
        def label = "base"
        def tagged(i: Int): Tag = new Tag {
          def label = "ref" + i
          def tagged(j: Int): Tag = base.tagged(j)
        }
      }
      outer.tagged(1).tagged(2).label
    }
  }

  // ---- 6: a symbolic extractor and a non-stable def of the same name ------
  class Nd(val s: String) {
    final def :@(t: Int): Nd = new Nd(s + t)
  }

  object NdOps {
    object :@ {
      def unapply(n: Nd): Option[(Nd, Int)] = Some((n, n.s.length))
    }
  }

  import NdOps._

  class Sub(s: String) extends Nd(s) {
    // `:@` is here also an inherited *method*, but a method is not a candidate
    // in the function position of a constructor pattern.
    def viaVal: Int = {
      val _ :@ n = (new Nd("abc")): @unchecked
      n
    }
    def viaCase: Int = (new Nd("abcd")) match {
      case _ :@ n => n
    }
    def viaMethod: String = (this :@ 5).s
  }
}

object Main {
  import implfind._

  implicit val jtInt: JT[Int] with BB[Int] = new JT[Int] with BB[Int] {
    def name = "jt-int"
  }

  // ---- 8: collection.Map --------------------------------------------------
  def viaCollMap(m: scala.collection.Map[String, Int]): String =
    s"${m.contains("a")} ${m("a")} ${m.get("b")} ${m.getOrElse("b", 9)}"

  def main(args: Array[String]): Unit = {
    println(Cake.viaBound(1))
    println(Cake.viaSelf(1))
    println(Cake.viaComponent(1))
    println(Cake.viaComponentJT(1))
    println(new Prot {}.viaTrait)
    println(new ProtC().viaClass)
    println(new UsesInner().a)
    println(new UsesInner().b)
    println(Anon.run)
    println(new Sub("z").viaVal)
    println(new Sub("z").viaCase)
    println(new Sub("z").viaMethod)

    // ---- 7: a Java Object return type -------------------------------------
    val props = new java.util.Properties()
    props.put("k", "v")
    println(props.get("k") ne null)
    println(props.get("nope") eq null)

    println(viaCollMap(Map("a" -> 1)))
  }
}
