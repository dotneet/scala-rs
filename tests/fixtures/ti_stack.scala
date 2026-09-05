// The stackable half: `Stacked`'s `label` runs Twice, then Loud, then Plain,
// and each `abstract override` layer reaches the next one through the
// `tilib$…$$super$label` accessor this class has to implement.
import tilib._

class Plain extends Base { def label = "b" }

class Stacked extends Plain with Loud with Twice

object Main {
  def main(args: Array[String]): Unit = println(new Stacked().label)
}
