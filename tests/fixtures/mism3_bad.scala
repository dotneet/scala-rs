// The third slice must not turn its relaxations into silence. scalac 2.13.16
// rejects every definition below (the wording differs).

// A protected member is still out of reach from outside the defining class's
// template, even for a subclass, when the prefix is an unrelated instance.
class P3 {
  protected def secret: Int = 1
}
class Q3 extends P3 {
  // nsc: method secret in class P3 cannot be accessed as a member of P3
  def peek(other: P3): Int = other.secret
}

// `this.type` is the receiver, which is not the same as the class applied to
// any arguments the caller likes.
class Cell3[T](val v: T) {
  def self3: this.type = this
}
object Bad3 {
  def wrong(c: Cell3[Int]): Cell3[String] = c.self3
}
