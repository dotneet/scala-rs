trait Show[A] extends java.io.Serializable {
  def show(a: A): String
}

trait Instances {
  def showOption[A](implicit A: Show[A]): Show[Option[A]] = {
    case Some(a) => "Some(" + A.show(a) + ")"
    case None => "None"
  }
}

object Main extends Instances {
  def roundTrip(value: AnyRef): AnyRef = {
    val bytes = new java.io.ByteArrayOutputStream()
    val out = new java.io.ObjectOutputStream(bytes)
    out.writeObject(value)
    out.close()
    val in = new java.io.ObjectInputStream(
      new java.io.ByteArrayInputStream(bytes.toByteArray)
    )
    in.readObject()
  }

  def main(args: Array[String]): Unit = {
    val show = showOption[Int](new Show[Int] {
      def show(a: Int): String = a.toString
    })
    println(roundTrip(show).asInstanceOf[Show[Option[Int]]].show(Some(7)))
  }
}
