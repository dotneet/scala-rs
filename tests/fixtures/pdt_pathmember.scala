// Path-dependent type members: `p.T` is the declaration `T` seen through the
// path `p`, and it keeps that prefix.
//
// Four shapes, all of them written the way the library that needed them
// writes it:
//
//  1. `Steps.wrap` -- cats' `Eval#flatMap`: an anonymous subclass fixes its
//     own abstract member to the path member of a value it captured
//     (`type Start = c.Start`), then assigns `c.start` to a `val` declared at
//     its own `Start`.
//  2. `Fwd.two` -- `scala.concurrent.duration.DurationConversions`: a method
//     whose result names its own implicit parameter's path (`ev.R`) forwards
//     to another one, which means the *argument's* path at that call.
//  3. `Rep#pair` -- cats' `Representable#compose`: an anonymous subclass whose
//     `type Rp` is built out of the enclosing instance's own member, named
//     through the self alias.
//  4. `Dep.take` / `Dep.lazily` -- `pos/t1569` and `run/t6443-by-name`: a
//     later parameter clause names a parameter of the clause just applied,
//     and the argument settles what `c.V` is.
//  5. `Main.use` -- a member selected through a path and handed back to the
//     same path's method.

trait Box {
  type T
  def get: T
  def show(t: T): String
}

class IntBox(n: Int) extends Box {
  type T = Int
  def get: Int = n
  def show(t: Int): String = "int:" + t
}

abstract class Step {
  type Start
  val start: () => Start
  val run: Start => String
}

object Steps {
  def wrap(c: Step): Step =
    new Step {
      type Start = c.Start
      val start: () => Start = c.start
      val run: Start => String = (s: c.Start) => "[" + c.run(s) + "]"
    }
}

trait Conv[C] {
  type R
  def convert(c: C): R
}

object Conv {
  implicit val intConv: Conv[Int] { type R = String } =
    new Conv[Int] {
      type R = String
      def convert(c: Int): String = "n" + c
    }
}

object Fwd {
  def one[C](c: C)(implicit ev: Conv[C]): ev.R = ev.convert(c)
  def two[C](c: C)(implicit ev: Conv[C]): ev.R = one(c)
}

trait Rep { self =>
  type Rp
  def tag: String
  def pair(other: Rep): Rep = new Rep {
    type Rp = (self.Rp, other.Rp)
    def tag: String = self.tag + "&" + other.tag
  }
}

// A later parameter clause naming a parameter of the clause just applied:
// `pos/t1569` and `run/t6443-by-name` in the scala/scala corpus.
class Cell { type V }

object Dep {
  def take(x: Int)(c: Cell)(v: c.V): String = "v" + v
  def lazily(c: Cell)(v: => c.V): String = "lz" + v
}

object Main {
  def use(b: Box): String = b.show(b.get)

  def main(args: Array[String]): Unit = {
    println(use(new IntBox(7)))

    val st = new Step {
      type Start = Int
      val start: () => Int = () => 3
      val run: Int => String = (s: Int) => "s" + s
    }
    val w = Steps.wrap(st)
    println(w.run(w.start()))

    val two: String = Fwd.two(5)
    println(two)

    println(Dep.take(1)(new Cell { type V = String })("z"))
    println(Dep.lazily(new Cell { type V = Int })(9))

    val r1 = new Rep { type Rp = Int; def tag: String = "a" }
    val r2 = new Rep { type Rp = String; def tag: String = "b" }
    println(r1.pair(r2).tag)
  }
}
