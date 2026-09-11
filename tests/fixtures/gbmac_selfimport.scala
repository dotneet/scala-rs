// gitbucket's component shape without slick: `trait X { self: Profile =>
// import profile.api._ … }`, with a `trait Profile` and an `object Profile`
// of the same name. Real scalac 2.13.16 prints what
// `expected/gbmac_selfimport.txt` holds; each part was a silent miscompile in
// scala-rs (run under `-Xverify:all`).
package gbmacsi

import scala.language.implicitConversions

class Tagger(val s: String)

trait Api {
  def name: String
  class Wrapped(val s: String) { def shout: String = s + "!" + name }
  // A view in an `object` that belongs to the instance.
  object obj { implicit def wrap(s: String): Wrapped = new Wrapped(s) }
  // A view in a trait-typed `val`, slick's `profile.api` shape.
  trait API { implicit def wrap2(s: String): Wrapped = new Wrapped(s + "2") }
  val api: API = new API {}
}

trait Profile {
  val profile: Api
  def currentDate: String = "today"
  implicit def tagger: Tagger = new Tagger("tag of " + profile.name)
}

trait ProfileProvider { self: Profile =>
  lazy val profile: Api = new Api { def name = "object" }
}

// 1. A service names a member of `trait Profile` through the object, as
// gitbucket's services do (`import gitbucket.core.model.Profile.currentDate`).
// That import used to be taken as the receiver of every `profile` a
// component reads through its self type -- the object's, not `this`'s.
class Service {
  import gbmacsi.Profile.currentDate
  def fromObject: String = currentDate
}

trait Comp { self: Profile =>
  def whose: String = profile.name
  // A class nested in the component, where gitbucket's table classes sit:
  // the implicit comes from the enclosing component's self type.
  class Row {
    def tagged(implicit t: Tagger): String = t.s
    def show: String = tagged + " / " + profile.name
  }
}

// 2. A view imported from an `object` of the self type's member: the call
// needs `profile.obj` as its receiver.
trait Views { self: Profile =>
  import profile.obj._
  def viaObject: String = "x".shout
  // 3. The import's root name shadowed where the view is used: the import
  // still means `this`'s `profile`.
  def shadowed(profile: Int): String = "s".shout + profile
}

// The same through slick's trait-typed `api`.
trait Views2 { self: Profile =>
  import profile.api._
  def viaApi: String = "y".shout
  def shadowed2(profile: Int): String = "t".shout + profile
}

object Profile extends ProfileProvider with Profile with Comp with Views with Views2

object Main {
  def main(args: Array[String]): Unit = {
    val other = new Profile with Comp with Views with Views2 {
      lazy val profile: Api = new Api { def name = "other" }
    }
    println(new Service().fromObject)
    println(other.whose)
    println(new other.Row().show)
    println(other.viaObject)
    println(other.shadowed(7))
    println(other.viaApi)
    println(other.shadowed2(8))
    println(Profile.whose + " | " + new Profile.Row().show + " | " + Profile.viaObject)
  }
}
