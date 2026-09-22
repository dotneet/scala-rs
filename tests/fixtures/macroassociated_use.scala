object Main {
  def summon[A](implicit value: Associated[A]): Associated[A] = value
  def main(args: Array[String]): Unit = println(summon[Int].value)
}
