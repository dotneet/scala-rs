// gitbucket's `GitBucketCoreModule.scala` without gitbucket. An anonymous class
// inside a *parent constructor argument* has its bodies completed where it
// stands (`type_local_template`), and the later passes do not revisit it -- so
// the signature pass froze a body typed before `gz2_convlib.scala`, which comes
// second on the command line, had any signatures: `base.pick` was "value pick
// is not a member of String", that complaint was dropped with the rest of the
// signature pass's, and the local `val` kept an `Error` type that only the
// backend noticed ("unresolved apply").
import gz2conv.Gz2Conv

abstract class Gz2Job { def run(base: String): Unit }
class Gz2Batch(val name: String, val jobs: Gz2Job*)
class Gz2Suite(val id: String, val batches: Gz2Batch*)

object Gz2ConvUse
    extends Gz2Suite(
      "suite",
      new Gz2Batch(
        "batch",
        new Gz2Job {
          override def run(base: String): Unit = {
            import Gz2Conv._
            val list = base.pick("xy") { n => n * 2 }
            list.foreach { x => println(x) }
          }
        }
      )
    ) {
  def main(args: Array[String]): Unit = batches.head.jobs.head.run("abcd")
}
