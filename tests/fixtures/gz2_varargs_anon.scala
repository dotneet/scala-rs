// An anonymous class passed to a *repeated* parameter. The repeated parameter's
// field symbol has the type it has inside the body (`Seq[T]`), which is the
// expected type of no single argument -- nsc's `formalTypes` expands `T*` into
// one `T` per argument. Handed `Seq[Gz2Step]`, the anonymous class failed to
// conform, `type_apply_in` rolled the argument back to its pristine clone and
// typed it a second time, and the second pass re-entered the template's members
// with fresh symbols whose signatures the node-id-keyed `sig_done` then
// skipped: "not found: value a" inside the class's own method, and "object
// creation impossible" for the class.
abstract class Gz2Step { def run(a: Int): Int }
class Gz2Phase(val name: String, val steps: Gz2Step*)
class Gz2Plan(val id: String, val phases: Gz2Phase*)

object Gz2VarargsAnon
    extends Gz2Plan(
      "plan",
      new Gz2Phase(
        "one",
        new Gz2Step {
          override def run(a: Int): Int = { val b = a + 1; b * 2 }
        },
        new Gz2Step {
          override def run(a: Int): Int = a - 1
        }
      )
    ) {
  val direct = new Gz2Phase("two", new Gz2Step { override def run(a: Int): Int = a * 10 })

  def main(args: Array[String]): Unit = {
    println(phases.head.steps.map(_.run(3)).mkString(","))
    println(direct.steps.head.run(3))
  }
}
