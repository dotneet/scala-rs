#!/usr/bin/env python3
"""Library-heavy workloads for comparing scala-rs with scalac 2.13.16.

The kinds scalac used to win: code over `cats.syntax.all._` (syntax views,
type-class instances, `Validated`/`Ior`/`Kleisli`), cats-effect `IO`, monad
transformers, and very large literal collections; with fs2 and circe's
hand-written codecs beside them. `docs/performance.md` ("Library-heavy code
against scalac") records the timings; `tests/library_bench.sh` runs them.

Usage: tests/library_bench_gen.py OUT_DIR [KIND...]   (all kinds by default)

Each kind is a directory of sources, a `Main`, and a `cp.txt` naming the
libraries it needs (`cats`, `ce`, `fs2`, `circe`), which `library_bench.sh`
resolves from the Coursier cache. `NF` sets the file count (20).
Generation is deterministic.
"""
import os, sys
NF = int(os.environ.get('NF', '20'))


def w(d, n, t):
    os.makedirs(d, exist_ok=True)
    open(os.path.join(d, n), 'w').write(t)


def main(d, body, imports=""):
    w(d, "Main.scala", f"{imports}\nobject Main {{\n  def main(args: Array[String]): Unit = {{\n{body}\n  }}\n}}\n")


def cp(d, libs):
    w(d, "cp.txt", " ".join(libs))


# cats.syntax.all._ over std types: traverse, mapN, foldMap, |+|, Validated
def catsyntax(d):
    for f in range(NF):
        L = [f"package cs{f}", "import cats._", "import cats.data._", "import cats.syntax.all._", f"object C{f} {{"]
        for m in range(60):
            L.append(f"  def m{m}(xs: List[Int], o: Option[String], e: Either[String, Int]): (Option[List[Int]], Either[String, Int], Int, ValidatedNel[String, Int], String) = {{")
            L.append(f"    val a = xs.traverse(x => if (x > {m}) Option(x) else Option(x + 1))")
            L.append(f"    val b = (e, o.toRight(\"none\").map(_.length)).mapN(_ + _ + {m})")
            L.append(f"    val c = xs.foldMap(x => x * {m % 7 + 1}) |+| xs.combineAll")
            L.append(f"    val v = (xs.headOption.toValidNel(\"empty\"), e.toValidatedNel).mapN(_ * _)")
            L.append(f"    val s = xs.map(_.show).mkString |+| o.orEmpty")
            L.append(f"    (a, b, c, v, s)")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, '    println(cs0.C0.m1(List(1,2,3), Some("ab"), Right(4)))')
    cp(d, ["cats"])

# cats.data: Validated, NonEmptyList, Ior, Kleisli, groupByNel
def catsdata(d):
    for f in range(NF):
        L = [f"package cd{f}", "import cats._", "import cats.data._", "import cats.syntax.all._", f"object D{f} {{"]
        for m in range(50):
            L.append(f"  def m{m}(xs: List[Int]): (ValidatedNel[String, List[Int]], Option[NonEmptyList[Int]], Ior[String, Int], Int, Map[Int, List[Int]]) = {{")
            L.append(f"    val v = xs.traverse(x => if (x > -{m}) x.validNel[String] else s\"bad $x\".invalidNel[Int])")
            L.append(f"    val n = NonEmptyList.fromList(xs).map(_.map(_ * {m}).sortBy(-_))")
            L.append(f"    val i = xs.foldLeft(Ior.right[String, Int](0))((acc, x) => acc.flatMap(a => if (x > 100) Ior.both(\"big\", a + x) else Ior.right(a + x)))")
            L.append(f"    val k = Kleisli((x: Int) => Option(x + {m})).andThen(Kleisli((y: Int) => Option(y * 2))).run(xs.sum).combineAll")
            L.append(f"    val g = xs.groupByNel(_ % 3).map {{ case (k2, v2) => k2 -> v2.toList }}")
            L.append(f"    (v, n, i, k, g)")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, "    println(cd0.D0.m1(List(3, 1, 2)))")
    cp(d, ["cats"])

# cats-effect IO: Ref, parTraverse, Resource, error handling
def catseffect(d):
    for f in range(NF):
        L = [f"package ce{f}", "import cats.effect._", "import cats.syntax.all._", "import scala.concurrent.duration._", f"object E{f} {{"]
        for m in range(50):
            L.append(f"  def m{m}(n: Int): IO[(Int, String)] = for {{")
            L.append(f"    ref <- Ref.of[IO, Int](n)")
            L.append(f"    _   <- (1 to {m % 5 + 2}).toList.traverse_(i => ref.update(_ + i * {m}))")
            L.append(f"    xs  <- List(1, 2, 3).parTraverse(i => IO.pure(i * n))")
            L.append(f"    r   <- Resource.make(IO.pure(\"r{m}\"))(_ => IO.unit).use(s => IO(s + xs.sum))")
            L.append(f"    v   <- ref.get")
            L.append(f"    e   <- IO.raiseError[Int](new Exception(\"x\")).handleErrorWith(_ => IO.pure(v))")
            L.append(f"  }} yield (e, r)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, '    import cats.effect.unsafe.implicits.global\n    println(ce0.E0.m1(3).unsafeRunSync())')
    cp(d, ["cats", "ce"])

# EitherT / OptionT / StateT over IO in for-comprehensions
def monadtrans(d):
    for f in range(NF):
        L = [f"package mt{f}", "import cats.data._", "import cats.effect._", "import cats.syntax.all._", f"object T{f} {{"]
        for m in range(40):
            L.append(f"  def m{m}(n: Int): IO[Either[String, (Int, String)]] = (for {{")
            L.append(f"    a <- EitherT.rightT[IO, String](n + {m})")
            L.append(f"    b <- EitherT.fromOption[IO](Option(a).filter(_ > 0), \"neg\")")
            L.append(f"    c <- EitherT(IO.pure(if (b % 2 == 0) Right(b * 2) else Right(b): Either[String, Int]))")
            L.append(f"    d <- OptionT.fromOption[IO](Some(c.toString)).getOrElse(\"none\").attemptT.leftMap(_.getMessage)")
            L.append(f"    e <- EitherT.liftF[IO, String, Int](StateT.modify[IO, Int](_ + c).runS(b))")
            L.append(f"  }} yield (e, d)).value")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, "    import cats.effect.unsafe.implicits.global\n    println(mt0.T0.m1(3).unsafeRunSync())")
    cp(d, ["cats", "ce"])

# fs2 Stream pipelines
def fs2s(d):
    for f in range(NF):
        L = [f"package fs{f}", "import cats.effect._", "import fs2._", f"object S{f} {{"]
        for m in range(50):
            L.append(f"  def m{m}(n: Int): IO[List[String]] = Stream.range(0, n).covary[IO].map(_ * {m}).filter(_ % 3 != 0)"
                     f".evalMap(i => IO.pure(i.toString)).chunkN(2).map(_.toList.mkString(\"-\")).zipWithIndex"
                     f".map {{ case (s, i) => s + \":\" + i }}.take({m % 7 + 2}).compile.toList")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, '    import cats.effect.unsafe.implicits.global\n    println(fs0.S0.m1(10).unsafeRunSync())')
    cp(d, ["cats", "ce", "fs2"])

# circe codecs written with forProduct5 (no macros)
def circeprod(d):
    for f in range(NF):
        L = [f"package cp{f}", "import io.circe._", "import io.circe.syntax._", f"object C{f} {{"]
        for c in range(20):
            L.append(f"  final case class R{c}(a: Int, b: String, c: Option[Double], d: List[Int], e: Map[String, Int])")
            L.append(f"  implicit val enc{c}: Encoder[R{c}] = Encoder.forProduct5(\"a\", \"b\", \"c\", \"d\", \"e\")(r => (r.a, r.b, r.c, r.d, r.e))")
            L.append(f"  implicit val dec{c}: Decoder[R{c}] = Decoder.forProduct5(\"a\", \"b\", \"c\", \"d\", \"e\")(R{c}.apply)")
            L.append(f"  def rt{c}(r: R{c}): Either[Error, R{c}] = r.asJson.as[R{c}].left.map(e => e: Error)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, "    println(cp0.C0.rt1(cp0.C0.R1(1, \"x\", Some(2.0), List(3), Map(\"k\" -> 4))))")
    cp(d, ["circe", "cats"])

# very large literal List / Map / Vector expressions (lub over thousands of elements)
def biglits(d):
    for f in range(NF):
        L = [f"package bl{f}", "sealed trait T; case object A extends T; case object B extends T; final case class Cc(i: Int) extends T", f"object L{f} {{"]
        L.append("  def ints = List(" + ", ".join(str(i) for i in range(2000)) + ")")
        L.append("  def mixed = List(" + ", ".join(["A", "B", "Cc(1)"][i % 3] for i in range(1500)) + ")")
        L.append("  def m = Map(" + ", ".join(f'"k{i}" -> {i}' for i in range(1000)) + ")")
        L.append("  def v = Vector(" + ", ".join(f"({i}, \"s{i}\", {i}.0)" for i in range(400)) + ")")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    main(d, "    println((bl0.L0.ints.sum, bl0.L0.mixed.size, bl0.L0.m.size, bl0.L0.v.size))")
    cp(d, [])


KINDS = {k: v for k, v in globals().items() if callable(v) and k not in ('w', 'main', 'cp')}
out = sys.argv[1]
for k in (sys.argv[2:] or KINDS):
    KINDS[k](os.path.join(out, k))
    print("generated", k)
