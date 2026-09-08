// Compiled *before* `Main_1.scala` in the same run, and that order is the
// whole point of this file: naming `Slickish.later` enters
// `scala.concurrent.ExecutionContext` as a stub, so `Main_1.scala`'s
// `import scala.concurrent.ExecutionContext.Implicits.global` walks the
// prefix from the class rather than reading the class file for the first
// time. See root 3 in `Lib_1.scala`.
import gboptlib.Slickish

object Uses {
  def twice(implicit ec: scala.concurrent.ExecutionContext): Int =
    Slickish.later(Slickish.later(1))
}
