// The application half of `crates/cli/tests/implclassbin.rs`: compiled
// against `ic_lib`'s class files, and run. Every selection below is one a
// conversion read out of a `-cp` pickle has to supply.
// Named one at a time, not `import iclib._`: a *package* wildcard leaves
// `import profile.api._` resolving to nothing at all, implicits or not, and
// that is a separate defect (docs/gitbucket.md, "A package wildcard hides the
// prefix of a later member import"). gitbucket writes explicit imports.
import iclib.Prof
import iclib.Target
import iclib.Holder
import iclib.Flat

// gitbucket's shape: an abstract `val` of the profile's type, with the API
// wildcard-imported from it (`val profile: BlockingJdbcProfile;
// import profile.blockingApi._`).
trait Client {
  val profile: Prof
  import profile.api._

  def bumped(t: Target): Int = t.bump
  def widened(t: Target): String = t.wide
}

object Main extends Client {
  val profile: Prof = Holder

  import Flat._

  def main(args: Array[String]): Unit = {
    val t = new Target(41)
    println(bumped(t))
    println(widened(t))
    // The same conversions through a concrete object path.
    locally {
      import Holder.api._
      println(t.bump)
      println(t.wide)
      // Applied explicitly: this compiled even when the view did not, so a
      // regression that loses only the `implicit` flag still prints here.
      println(RichTarget(t).bump)
    }
    println("hello".shout)
  }
}
