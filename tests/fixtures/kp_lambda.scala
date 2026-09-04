// kind-projector's type-lambda syntax, behind `-Ykind-projector`.
//
// kind-projector is a compiler *plugin*, not Scala: nsc rejects every type
// written below unless the plugin is on its classpath, and so does this
// compiler without the flag (`crates/cli/tests/kindproj.rs` pins that too).
// The expected output is what
// `scalac -Xplugin:kind-projector_2.13.16-0.13.3.jar` prints for this file.
//
// Each case is one of the plugin's documented forms, and cats writes the first
// six of them thousands of times.
object Main {
  trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }
  trait Bi[F[_, _]] { def show(x: F[Int, String]): String }
  trait Over[F[_[_]]] { def run(x: F[Box]): String }
  // A covariant slot, because a lambda declared covariant does not fit an
  // invariant `F[_]` -- scalac reports "covariant type α occurs in invariant
  // position" for `Functor[λ[`+α` => Box[α]]]`.
  trait CoF[F[+_]] { def id[A](fa: F[A]): F[A] = fa }

  final case class Box[A](value: A)
  final case class Pair[A, B](a: A, b: B)
  final case class Wrap[F[_]](fa: F[Int])
  final case class CBox[+A](value: A)

  // (1) `*` in the last position: `Either[E, *]`, the shape cats writes most.
  val p1: Functor[Pair[String, *]] = new Functor[Pair[String, *]] {
    def map[A, B](fa: Pair[String, A])(f: A => B): Pair[String, B] = Pair(fa.a, f(fa.b))
  }

  // (2) `*` in the first position.
  val p2: Functor[Pair[*, String]] = new Functor[Pair[*, String]] {
    def map[A, B](fa: Pair[A, String])(f: A => B): Pair[B, String] = Pair(f(fa.a), fa.b)
  }

  // (3) two placeholders, filled left to right.
  val p3: Bi[Pair[*, *]] = new Bi[Pair[*, *]] {
    def show(x: Pair[Int, String]): String = x.a.toString + "/" + x.b
  }

  // (4) reordered parameters: only the `λ` form can say this.
  val p4: Bi[λ[(α, β) => Pair[β, α]]] = new Bi[λ[(α, β) => Pair[β, α]]] {
    def show(x: Pair[String, Int]): String = x.a + "\\" + x.b.toString
  }

  // (5) a parenthesised tuple: `(A0, *)`.
  val p5: Functor[(String, *)] = new Functor[(String, *)] {
    def map[A, B](fa: (String, A))(f: A => B): (String, B) = (fa._1, f(fa._2))
  }

  // (6) a function type is an application of `FunctionN`, so `A => *` is a
  // lambda over the result and `* => *` is a lambda over both sides.
  val p6: Functor[Int => *] = new Functor[Int => *] {
    def map[A, B](fa: Int => A)(f: A => B): Int => B = (i: Int) => f(fa(i))
  }
  val p7: Bi[* => *] = new Bi[* => *] {
    def show(x: Int => String): String = x(7)
  }

  // (7) `λ` repeating its parameter, which the `*` form cannot express.
  val p8: Functor[λ[α => Pair[α, α]]] = new Functor[λ[α => Pair[α, α]]] {
    def map[A, B](fa: Pair[A, A])(f: A => B): Pair[B, B] = Pair(f(fa.a), f(fa.b))
  }

  // (8) the `Lambda` spelling, with a nested body.
  val p9: Functor[Lambda[a => Box[Box[a]]]] = new Functor[Lambda[a => Box[Box[a]]]] {
    def map[A, B](fa: Box[Box[A]])(f: A => B): Box[Box[B]] = Box(Box(f(fa.value.value)))
  }

  // (9) a higher-kinded `λ` parameter.
  val p10: Over[λ[F[_] => Wrap[F]]] = new Over[λ[F[_] => Wrap[F]]] {
    def run(x: Wrap[Box]): String = x.fa.toString
  }

  // (10) variance, which is written by backquoting the whole name.
  val p11: CoF[λ[`+α` => CBox[α]]] = new CoF[λ[`+α` => CBox[α]]] {}

  // (11) a placeholder binds to the *innermost* enclosing application: this is
  // `Wrap[[a] => Box[a]]`. Binding it to `Wrap[…]` instead would make the
  // whole thing a type constructor, and `p12` would not typecheck.
  val p12: Wrap[Box[*]] = Wrap[Box[*]](Box(12))

  // (12) variance on a placeholder.
  val p13: CoF[CBox[+*]] = new CoF[CBox[+*]] {}

  def main(args: Array[String]): Unit = {
    println(p1.map(Pair("a", 1))(_ + 1))
    println(p2.map(Pair(1, "b"))(_ + 1))
    println(p3.show(Pair(3, "c")))
    println(p4.show(Pair("d", 4)))
    // Printed field by field: `Tuple2.toString` is scala-library's, and the
    // private-runtime mode of this compiler has its own stand-in.
    val t5 = p5.map(("e", 5))(_ + 1)
    println(t5._1 + t5._2.toString)
    println(p6.map((i: Int) => i * 2)(_ + 1)(3))
    println(p7.show((i: Int) => "n" + i))
    println(p8.map(Pair(8, 9))(_ + 1))
    println(p9.map(Box(Box(10)))(_ + 1))
    println(p10.run(Wrap(Box(11))))
    println(p11.id(CBox(11)).value)
    println(p12.fa.value)
    println(p13.id(CBox(13)).value)
  }
}
