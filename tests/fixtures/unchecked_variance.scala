import scala.annotation.unchecked.uncheckedVariance

class Inv[A](val value: A)

class Q[+A] {
  def enqueue(x: A @uncheckedVariance): Int = 1
}

class Box[+A](val inner: Inv[A @uncheckedVariance]) {
  def get: A = inner.value
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Q[Int].enqueue(1))
    println(new Box(new Inv(41)).get)
  }
}
