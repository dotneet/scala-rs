// SAM literals of traits read from the class path (sam_mixin_cp_main.scala
// converts to them). Their interfaces declare the mixin setter of a `val`
// and the setter of a `var` abstract; the pickle does not, and nsc's
// `samOf` reads the pickle.
package lib
object Log { var lines = List.empty[String]; def add(s: String): Unit = lines = s :: lines }
trait OnlyVal { val tag = "t"; def run(i: Int): Int }
trait OnlyVar { var hits = 0; def run(i: Int): Int }
trait OnlyLazy { lazy val l = { Log.add("lazy"); 5 }; def run(i: Int): Int }
trait Mixed {
  Log.add("init " + base)
  val base = 10
  var count = base
  lazy val label = { Log.add("label"); "c" + base }
  def step(x: Int): Int
  def tick(): Int = { count = step(count); count }
}
