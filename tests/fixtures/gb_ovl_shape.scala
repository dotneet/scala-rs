// Overload resolution against a function literal whose parameter types are
// not inferred yet. nsc filters the alternatives on the literal's *shape*
// first (`Infer.shapeType`): the parameter types are unknown, but the arity
// is written in the source, and that alone separates a `Function1` parameter
// from a `Function2` one.
//
// gitbucket's `Authenticator.scala` is this pair, four times over
// (`ownerOnly` / `referrersOnly` / `readableUsersOnly` / `writableUsersOnly`),
// and every caller of them was `ambiguous overload`. See docs/gitbucket.md.

class Repo(val nm: String)

class Auth {
  // The two alternatives differ in nothing a caller writes except the arity
  // of the literal it passes.
  def only(action: Repo => Any): String = "1:" + action(new Repo("r"))
  def only[T](action: (T, Repo) => Any): T => String =
    (form: T) => "2:" + action(form, new Repo("r"))
}

class Shapes {
  // A single alternative still takes a `{ case … }` literal by tupling: one
  // written parameter, a `Function2` parameter type. The shape filter must
  // not reject this -- it only ever narrows a set that is already ambiguous.
  def tupled(f: (Int, String) => String): String = f(7, "s")

  // A `{ case … }` literal is a `PartialFunction`, i.e. arity one, so it
  // picks the `PartialFunction` alternative over the `Function2` one.
  def pick(f: PartialFunction[Int, String]): String = "pf:" + f(3)
  def pick(f: (Int, String) => String): String = "f2:" + f(1, "x")

  // Overloading on the literal's arity where neither alternative is generic.
  def both(f: Int => String): String = "one:" + f(1)
  def both(f: (Int, Int) => String): String = "two:" + f(1, 2)
}

object Main {
  def main(args: Array[String]): Unit = {
    val a = new Auth
    println(a.only { r => r.nm })
    println(a.only[String] { (form, r) => form + "/" + r.nm }("p"))
    // The type argument is inferred from the expected type of the result.
    val f: String => String = a.only { (form: String, r: Repo) => form + "!" + r.nm }
    println(f("q"))

    val s = new Shapes
    println(s.tupled { case (n, str) => str + n })
    println(s.pick { case n => "n" + n })
    println(s.both { n => "a" + n })
    println(s.both { (n, m) => "b" + (n + m) })
  }
}
