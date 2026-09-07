// Default arguments reached through a prefix the call site does not write.
//
// Every default here is *called*, and every call prints what it produced, so a
// getter that answers the wrong value cannot pass as a green test.
package da

object helpers {
  def avatar(name: String, size: Int, tooltip: Boolean = false): String =
    name + "/" + size + "/" + tooltip
}

trait AccountService {
  def getAccountByUserName(name: String, includeRemoved: Boolean = false): String =
    name + ":" + includeRemoved
  // A later clause's default reads earlier clauses' parameters, so its getter
  // takes the arguments that precede it.
  def label(user: String)(shown: String = "@")(suffix: String = shown + user): String =
    shown + user + "|" + suffix
}

trait Runner { def run(): String }

// The cake: `Ctl` does not extend `AccountService`, it only requires it.
trait Ctl { self: AccountService =>
  import helpers._

  // A member of a wildcard-imported *object*: the getter is
  // `helpers.avatar$default$3`, not `this.avatar$default$3`.
  def viaImport(): String = avatar("octocat", 16)

  // A member the self type contributes, named from inside an anonymous class:
  // the getter is `Ctl.this.getAccountByUserName$default$2`, reached through
  // the anonymous class's `$outer`, not off the anonymous class itself.
  def viaSelfTypeInAnon(): Runner = new Runner {
    def run(): String = getAccountByUserName("root")
  }

  def viaSelfTypeDirect(): String = getAccountByUserName("direct")

  def chained(): String = label("me")()()
}

trait ActivityService {
  def recordActivity(what: String, kind: String = "push"): String = what + "!" + kind
}

// A *compound* self type -- what every gitbucket controller writes. Only the
// first component used to be visible from inside, so `recordActivity`'s getter
// was "not a member of Wiki".
trait Wiki { self: AccountService with ActivityService =>
  def edit(): String = recordActivity("wiki") + " " + getAccountByUserName("w")
}

// A `def` inside a method body, with a default. Its getter is local to that
// body, so the default is spliced rather than selected off a receiver -- twirl
// writes gitbucket's templates this way.
object Local {
  def render(active: String): String = {
    def menuitem(path: String, count: Int = 0): String = path + count
    menuitem("files") + "|" + menuitem(active, 3)
  }
}

// The plain inherited case, which already worked: kept so a regression here
// is visible in the same fixture.
trait Svc { def get(name: String, all: Boolean = false): String = name + all }
trait Plain extends Svc { def use(): String = get("x") }

object Main extends Ctl with AccountService with Plain with Wiki with ActivityService {
  def main(args: Array[String]): Unit = {
    println(viaImport())
    println(helpers.avatar("qualified", 32))
    println(viaSelfTypeInAnon().run())
    println(viaSelfTypeDirect())
    println(chained())
    println(use())
    // An explicit argument still wins over the default.
    println(helpers.avatar("explicit", 8, true))
    println(getAccountByUserName("named", includeRemoved = true))
    println(edit())
    println(Local.render("branches"))
  }
}
