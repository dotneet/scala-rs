// Two linearisation faults that only showed up when the JVM tried to link.
//
//  1. A trait nested in a trait or class, mixed into something that reaches
//     the enclosing instance through an *enclosing object* rather than through
//     an `$outer` chain, left the interface's `<Trait>$$$outer()` accessor
//     unimplemented: `AbstractMethodError: Receiver class Main$$anon$1 does
//     not define or inherit ... 'abstract Outer Outer$Inner$$$outer()'`.
//
//  2. SLS 5.1.2: the superclass of a template is the superclass of its
//     linearisation, which a *trait* parent can supply. `trait Mid extends
//     Base; object Impl extends Mid` is `Impl$ extends Base implements Mid` in
//     nsc's own output; emitting `extends java/lang/Object` there put `Base`
//     out of reach of everything nested in `Impl`.
trait Outer {
  val tag: String = "outer"
  trait Inner {
    def describe: String = "inner of " + tag
  }
  class InnerClass {
    def describe: String = "innerclass of " + tag
  }
}

class Base {
  def base: String = "base"
}

trait Mid extends Base
trait OuterMid extends Outer

object Impl extends Mid {
  def report: String = base + "/impl"
}

object Host extends OuterMid {
  override val tag: String = "host"
  def anon: String = (new Inner {}).describe
  def named: String = (new InnerClass).describe
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Impl.report)
    println(Host.anon)
    println(Host.named)
  }
}
