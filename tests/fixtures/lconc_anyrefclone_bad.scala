// `AnyRef.clone` is `protected`: declaring it must not make it callable on an
// arbitrary receiver. scalac rejects all three of these ("method clone in class
// Object cannot be accessed ... prefix type ... does not conform to ... where
// the access takes place").
object Main {
  class F extends Cloneable
  class H extends Cloneable { def bad3(other: F): AnyRef = other.clone() }

  def bad1(): AnyRef = (new Object).clone()
  def bad2(f: F): AnyRef = f.clone()
}
