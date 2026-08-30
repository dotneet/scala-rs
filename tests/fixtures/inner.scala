// The reported bug: `object Main { trait Shape; class Circle extends Shape }`
// compiled with no `InnerClasses` attribute at all, so `getClass.getSimpleName`
// on a `Circle` returned the *binary* name `Main$Circle` instead of `Circle`.
object Main {
  trait Shape { def area: Double; def name: String = getClass.getSimpleName }
  class Circle(r: Double) extends Shape { def area = 3.14 * r * r }

  def main(args: Array[String]): Unit = {
    val c = new Circle(1.0)
    println(c.name)
    println(c.getClass.getSimpleName)
    println(c.getClass.isMemberClass)
    println(classOf[Shape].getSimpleName)
    println(classOf[Shape].isMemberClass)
    println(c.getClass.getEnclosingClass != null)
    println(c.getClass.getDeclaringClass != null)
  }
}
