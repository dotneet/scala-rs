// JVMS 4.7.9 `Signature` attributes, read back through java.lang.reflect.
//
// Everything the generic signature carries is invisible to `javap -p` and to
// the class loader, so the only honest check is to ask the JVM's own
// reflection for it: `Class#toGenericString`, `Method#toGenericString`,
// `Field#getGenericType`, `Class#getGenericSuperclass` and
// `#getGenericInterfaces` all read the attribute and fall back to the erased
// shape when it is missing.
//
// `e(x: Int)` is here for the other half of the claim: a member with no
// generic information must carry no attribute at all, so its generic string
// is its erased one.
//
// The last line is `@SerialVersionUID`, whose value nsc puts in a JVMS 4.7.2
// `ConstantValue` on a `private static final long serialVersionUID` -- the
// field `ObjectStreamClass.lookup` reads. The argument is written as an
// expression on purpose: nsc constant-folds it (`run/t6988`).
package sg

class Wrapper[X](x: X)
trait Bippy[A]

class Sub extends Wrapper[String]("x") with Bippy[Int]

class C[T] {
  val f: Wrapper[String] = null
  def a(w: Wrapper[Array[Int]]): Int = 0
  def b(w: Wrapper[Int]): Int = 0
  def c(t: T): T = t
  def d[U](u: U): Wrapper[U] = new Wrapper[U](u)
  def e(x: Int): Int = x
}

@SerialVersionUID(10L + 3L) class Ser extends java.io.Serializable

object Main {
  def show(cls: Class[_], name: String): Unit = {
    val ms = cls.getDeclaredMethods
    var i = 0
    while (i < ms.length) {
      if (ms(i).getName() == name) println(ms(i).toGenericString())
      i += 1
    }
  }

  def main(args: Array[String]): Unit = {
    val c = classOf[C[_]]
    println(c.toGenericString())
    show(c, "a")
    show(c, "b")
    show(c, "c")
    show(c, "d")
    show(c, "e")
    println(c.getDeclaredField("f").getGenericType().toString())
    val s = classOf[Sub]
    println(s.getGenericSuperclass().toString())
    println(s.getGenericInterfaces()(0).toString())
    println(java.io.ObjectStreamClass.lookup(classOf[Ser]).getSerialVersionUID())
  }
}
