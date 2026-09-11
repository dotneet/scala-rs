// `var x: T = _` default initialization for every type, vars in objects and
// traits, setters reached through a trait, and object vars mutated from
// elsewhere.
object Main {
  class Defaults {
    var i: Int = _; var l: Long = _; var d: Double = _; var f: Float = _; var b: Boolean = _
    var c: Char = _; var s: String = _; var by: Byte = _; var sh: Short = _; var u: Unit = _; var li: List[Int] = _
    override def toString = s"$i $l $d $f $b ${c.toInt} $s $by $sh $u $li"
  }
  object Global { var counter = 0; var name: String = _; def bump(): Int = { counter += 1; counter } }
  trait HasVar { var v: Int = 10 }
  class UsesVar extends HasVar { def inc(): Unit = v += 1 }
  trait AbstractVar { var av: String }
  class ConcreteVar extends AbstractVar { var av = "init" }
  class CustomSetter { private var _x = 0; def x = _x; def x_=(n: Int): Unit = { _x = if (n < 0) 0 else n } }
  def main(args: Array[String]): Unit = {
    val d = new Defaults
    println(d)
    d.i = 5; d.s = "set"; d.li = List(1); d.c = 'k'
    println(d)
    println(Global.counter + " " + Global.name)
    Global.bump(); Global.bump(); Global.name = "g"
    println(Global.counter + " " + Global.name)
    Global.counter += 10
    println(Global.counter)
    val u = new UsesVar; u.inc(); u.v *= 2
    println(u.v)
    val hv: HasVar = u; hv.v = -1
    println(u.v)
    val cv: AbstractVar = new ConcreteVar; cv.av += "+more"
    println(cv.av)
    val cs = new CustomSetter
    cs.x = 5; println(cs.x)
    cs.x = -3; println(cs.x)
  }
}
