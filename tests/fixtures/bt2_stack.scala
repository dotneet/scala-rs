// Three stackable layers with the traits on the *other* side of a class file:
// `Stacked.label` must run Twice, then Loud, then `Plain`'s own body. Each
// layer reaches the next through the `bt2lib$…$$super$label` accessor this
// class owes, and the middle one is a trait nothing in this run compiled.
// See `crates/cli/tests/traitclass.rs`.
import bt2lib._

class Plain extends Base { def label = "b" }

class Stacked extends Plain with Loud with Twice

object Main {
  def main(args: Array[String]): Unit = println(new Stacked().label)
}
