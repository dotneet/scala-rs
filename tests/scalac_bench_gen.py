#!/usr/bin/env python3
"""Synthetic workloads for comparing scala-rs with scalac 2.13.16.

Each kind is a directory of sources plus a `Main` whose output both
compilers' programs must agree on. `docs/performance.md` ("Across the board
against scalac") records the timings; `tests/scalac_bench.sh` runs them.

Usage: tests/scalac_bench_gen.py OUT_DIR [KIND...]   (all kinds by default)

`NF` sets the file count of the kinds that take one (20 by default) and
`SUFFIX` is appended to each directory name: the `_big` rows of the table
are `NF=100 SUFFIX=_big`, `typeclass_n200` is `NF=200 SUFFIX=_n200`.
Generation is deterministic.
"""
import os, sys, random
NF = int(os.environ.get('NF', '20'))
SUFFIX = os.environ.get('SUFFIX', '')
random.seed(1)
def w(dirn, name, text):
    os.makedirs(dirn, exist_ok=True); open(os.path.join(dirn, name), "w").write(text)

def main_obj(pkg, body):
    return f"object Main {{\n  def main(args: Array[String]): Unit = {{\n{body}\n  }}\n}}\n"

# 1. arith: many methods with arithmetic and local vals, 20 files x 300 methods
def arith(d):
    for f in range(NF):
        L=[f"package arith{f}", f"object A{f} {{"]
        for m in range(300):
            L.append(f"  def m{m}(x: Int, y: Long, z: Double): Double = {{ val a = x * {m} + y; val b = a.toDouble / (z + 1.0); if (b > {m}) b - x else b * 2 + y }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(arith0.A0.m1(1, 2L, 3.0))"))

# 2. coll: collection pipelines with lambdas & inference
def coll(d):
    for f in range(NF):
        L=[f"package coll{f}", f"object C{f} {{"]
        for m in range(150):
            L.append(f"  def m{m}(xs: List[Int], m: Map[String, Vector[Int]]): (Int, List[String], Map[Int, Seq[String]]) = {{")
            L.append(f"    val a = xs.filter(_ % {m%7+2} == 0).map(x => x * {m}).foldLeft(0)(_ + _)")
            L.append(f"    val b = m.toList.flatMap {{ case (k, v) => v.map(i => k + i) }}.sortBy(_.length).take({m%10+1})")
            L.append(f"    val c = b.zipWithIndex.groupBy(_._2 % 3).map {{ case (k, v) => k -> v.map(_._1).toSeq }}")
            L.append(f"    (a, b, c)")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(coll0.C0.m1(List(1,2,3,4), Map(\"a\" -> Vector(1,2))))"))

# 3. forcomp: for-comprehensions over Option/Either/List
def forcomp(d):
    for f in range(NF):
        L=[f"package forc{f}", f"object F{f} {{"]
        for m in range(150):
            L.append(f"  def m{m}(a: Option[Int], b: Either[String, Int], xs: List[Int]): (Option[Int], Either[String, Int], List[(Int, Int)]) = {{")
            L.append(f"    val o = for {{ x <- a; y <- Some(x + {m}) if y > 0; z = y * 2 }} yield z + x")
            L.append(f"    val e = for {{ x <- b; y <- Right(x + {m}); z <- if (y > 3) Right(y) else Left(\"no\") }} yield x + y + z")
            L.append(f"    val l = for {{ x <- xs; y <- xs if x < y; z = x * y }} yield (x, z)")
            L.append(f"    (o, e, l)")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(forc0.F0.m1(Some(1), Right(2), List(1,2,3)))"))

# 4. patmat: sealed ADT with nested pattern matches
def patmat(d):
    for f in range(NF):
        L=[f"package pm{f}", "sealed trait Expr", "final case class Num(v: Int) extends Expr", "final case class Add(a: Expr, b: Expr) extends Expr",
           "final case class Mul(a: Expr, b: Expr) extends Expr", "final case class Neg(a: Expr) extends Expr", "final case class Var(n: String) extends Expr",
           "final case class Let(n: String, v: Expr, b: Expr) extends Expr", "case object Zero extends Expr", f"object P{f} {{"]
        for m in range(120):
            L.append(f"  def m{m}(e: Expr, env: Map[String, Int]): Int = e match {{")
            L.append(f"    case Num(v) if v > {m} => v")
            L.append(f"    case Num(v) => v + {m}")
            L.append(f"    case Add(Num(0), b) => m{m}(b, env)")
            L.append(f"    case Add(a, Num(0)) => m{m}(a, env)")
            L.append(f"    case Add(Mul(a, b), Mul(c, d)) => m{m}(a, env) * m{m}(b, env) + m{m}(c, env) * m{m}(d, env)")
            L.append(f"    case Add(a, b) => m{m}(a, env) + m{m}(b, env)")
            L.append(f"    case Mul(Neg(a), Neg(b)) => m{m}(a, env) * m{m}(b, env)")
            L.append(f"    case Mul(a, b) => m{m}(a, env) * m{m}(b, env)")
            L.append(f"    case Neg(Neg(a)) => m{m}(a, env)")
            L.append(f"    case Neg(a) => -m{m}(a, env)")
            L.append(f"    case Var(n) => env.getOrElse(n, {m})")
            L.append(f"    case Let(n, v, b) => m{m}(b, env + (n -> m{m}(v, env)))")
            L.append(f"    case Zero => 0")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(pm0.P0.m1(pm0.Add(pm0.Num(1), pm0.Num(2)), Map()))"))

# 5. branches: long if/else and match chains returning different class types (lub heavy)
def branches(d):
    for f in range(NF):
        L=[f"package br{f}", "import scala.collection.immutable._", f"object B{f} {{"]
        kinds=["List(1)", "Vector(1)", "Nil", "Seq(1)", "IndexedSeq(1)", "LazyList(1)", "Queue(1)", "ArraySeq(1)", "Range(0, 1)", "List.empty[Int]"]
        for m in range(60):
            L.append(f"  def m{m}(i: Int) = i match {{")
            for k in range(len(kinds)):
                L.append(f"    case {k} => {kinds[(k+m)%len(kinds)]}")
            L.append(f"    case _ => if (i > {m}) Set(1) .toList else Map(1 -> 2).keys.toVector")
            L.append("  }")
            L.append(f"  def n{m}(i: Int) = if (i == 0) Some(List(1)) else if (i == 1) None else if (i == 2) Some(Vector(2)) else Option(Nil)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(br0.B0.m1(3)); println(br0.B0.n1(2))"))

# 6. literals: big collection literals
def literals(d):
    for f in range(10):
        L=[f"package lit{f}", f"object L{f} {{"]
        L.append("  val table = Map(" + ", ".join(f"\"k{i}\" -> ({i}, \"v{i}\", {i}.5)" for i in range(800)) + ")")
        L.append("  val list = List(" + ", ".join(f"({i}, if ({i} % 2 == 0) Some({i}) else None)" for i in range(800)) + ")")
        L.append("  val mixed = Seq(" + ", ".join(random.choice([f"{i}", f"{i}L", f"{i}.0", f"'{chr(97+i%26)}'"]) for i in range(600)) + ")")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(lit0.L0.table.size + lit0.L0.list.size + lit0.L0.mixed.size)"))

# 7. hierarchy: deep trait mixins with many members and overrides
def hierarchy(d):
    for f in range(15):
        L=[f"package hi{f}"]
        L.append("trait T0 { def a0: Int = 0; def name: String = \"t0\" }")
        for t in range(1, 40):
            L.append(f"trait T{t} extends T{t-1} {{ def a{t}: Int = a{t-1} + 1; override def name: String = \"t{t}\" + super.name.length }}")
        for c in range(40):
            mix=" with ".join(f"T{(c+j)%39+1}" for j in range(1,6))
            L.append(f"class C{c} extends T0 with {mix} {{ override def a0: Int = {c}; override def name: String = super.name + \"c{c}\" }}")
        L.append(f"object H{f} {{ def all = List(" + ", ".join(f"new C{c}" for c in range(40)) + ").map(_.name) }")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(hi0.H0.all.length)"))

# 8. typeclass: implicit type class derivation without macros (HList-ish, implicit chains)
def typeclass(d):
    base = """package tc
trait Show[A] { def show(a: A): String }
object Show {
  def apply[A](implicit s: Show[A]): Show[A] = s
  implicit val int: Show[Int] = (a: Int) => a.toString
  implicit val str: Show[String] = (a: String) => a
  implicit val dbl: Show[Double] = (a: Double) => a.toString
  implicit val bool: Show[Boolean] = (a: Boolean) => a.toString
  implicit def opt[A](implicit s: Show[A]): Show[Option[A]] = (a: Option[A]) => a.fold("none")(s.show)
  implicit def list[A](implicit s: Show[A]): Show[List[A]] = (a: List[A]) => a.map(s.show).mkString("[", ",", "]")
  implicit def vec[A](implicit s: Show[A]): Show[Vector[A]] = (a: Vector[A]) => a.map(s.show).mkString("<", ",", ">")
  implicit def map[K, V](implicit k: Show[K], v: Show[V]): Show[Map[K, V]] = (a: Map[K, V]) => a.map { case (x, y) => k.show(x) + "=" + v.show(y) }.mkString("{", ",", "}")
  implicit def tuple2[A, B](implicit a: Show[A], b: Show[B]): Show[(A, B)] = (t: (A, B)) => "(" + a.show(t._1) + "," + b.show(t._2) + ")"
  implicit def tuple3[A, B, C](implicit a: Show[A], b: Show[B], c: Show[C]): Show[(A, B, C)] = (t: (A, B, C)) => "(" + a.show(t._1) + "," + b.show(t._2) + "," + c.show(t._3) + ")"
  implicit def either[A, B](implicit a: Show[A], b: Show[B]): Show[Either[A, B]] = (e: Either[A, B]) => e.fold(a.show, b.show)
}
object syntax { implicit class ShowOps[A](private val a: A) extends AnyVal { def show(implicit s: Show[A]): String = s.show(a) } }
"""
    w(d, "Base.scala", base)
    types=["Int","String","Double","Boolean"]
    def rt(depth):
        if depth==0: return random.choice(types)
        k=random.randint(0,6)
        if k==0: return f"Option[{rt(depth-1)}]"
        if k==1: return f"List[{rt(depth-1)}]"
        if k==2: return f"Vector[{rt(depth-1)}]"
        if k==3: return f"Map[{rt(0)}, {rt(depth-1)}]"
        if k==4: return f"({rt(depth-1)}, {rt(depth-1)})"
        if k==5: return f"({rt(depth-1)}, {rt(0)}, {rt(depth-1)})"
        return f"Either[{rt(0)}, {rt(depth-1)}]"
    for f in range(NF):
        L=[f"package tc{f}", "import tc._", "import tc.syntax._", f"object T{f} {{"]
        for m in range(80):
            L.append(f"  def m{m}(x: {rt(3)}): String = x.show + Show[{rt(2)}].toString.length")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(tc0.T0.m1 _)"))

# 9. many small files
def smallfiles(d):
    for f in range(600):
        w(d, f"S{f}.scala", f"package small.p{f%20}\nfinal case class S{f}(a: Int, b: String) {{ def c: Int = a + b.length }}\nobject S{f} {{ def make(i: Int): S{f} = S{f}(i, i.toString) }}\n")
    w(d, "Main.scala", main_obj("", "    println(small.p0.S0.make(1).c)"))

# 10. javainterop: java.util collections, streams, StringBuilder overloads
def javainterop(d):
    for f in range(NF):
        L=[f"package jv{f}", "import java.util.{ArrayList, HashMap => JHashMap}", "import java.util.stream.Collectors", "import scala.jdk.CollectionConverters._", f"object J{f} {{"]
        for m in range(100):
            L.append(f"  def m{m}(n: Int): String = {{")
            L.append(f"    val sb = new java.lang.StringBuilder(); val al = new ArrayList[Integer](); val hm = new JHashMap[String, java.lang.Long]()")
            L.append(f"    var i = 0; while (i < n) {{ al.add(i * {m}); hm.put(\"k\" + i, i.toLong); sb.append(i).append(',').append({m}.5).append(\"x\"); i += 1 }}")
            L.append(f"    val s = al.stream().filter(x => x % 2 == 0).map[String](x => x.toString).collect(Collectors.joining(\";\"))")
            L.append(f"    sb.append(s).append(hm.asScala.values.map(_.longValue).sum).append(Math.max(n, {m})).append(Math.abs(-1.0 * n)).toString")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(jv0.J0.m1(5))"))

# 11. bigmethod: very long method bodies
def bigmethod(d):
    for f in range(8):
        L=[f"package bm{f}", f"object M{f} {{", "  def run(seed: Int): Int = {", "    var acc = seed"]
        for s in range(900):
            L.append(f"    acc = (acc * 31 + {s}) % 1000003; if (acc % 7 == {s%7}) acc += {s} else acc -= 1")
        L.append("    acc"); L.append("  }"); L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(bm0.M0.run(1))"))

# 12. generics: generic classes with bounds & variance, higher-kinded
def generics(d):
    for f in range(NF):
        L=[f"package gn{f}", "trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }",
           "trait Monad[F[_]] extends Functor[F] { def pure[A](a: A): F[A]; def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]; def map[A, B](fa: F[A])(f: A => B): F[B] = flatMap(fa)(a => pure(f(a))) }",
           "final case class Box[+A](a: A) { def map[B](f: A => B): Box[B] = Box(f(a)); def flatMap[B](f: A => Box[B]): Box[B] = f(a) }",
           "object Box { implicit val monad: Monad[Box] = new Monad[Box] { def pure[A](a: A) = Box(a); def flatMap[A, B](fa: Box[A])(f: A => Box[B]) = fa.flatMap(f) } }",
           f"object G{f} {{",
           "  def lift[F[_], A, B](fa: F[A])(f: A => B)(implicit F: Functor[F]): F[B] = F.map(fa)(f)",
           "  def seq[F[_], A](xs: List[F[A]])(implicit F: Monad[F]): F[List[A]] = xs.foldRight(F.pure(List.empty[A]))((fa, acc) => F.flatMap(fa)(a => F.map(acc)(a :: _)))",
           "  def cmp[A <: Comparable[A]](a: A, b: A): A = if (a.compareTo(b) > 0) a else b"]
        for m in range(120):
            L.append(f"  def m{m}(xs: List[Box[Int]]): Box[List[String]] = lift(seq(xs.map(b => lift(b)(_ + {m}))))(l => l.map(i => cmp(i.toString, \"{m}\")))")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(gn0.G0.m1(List(gn0.Box(1))))"))

# 13. strings: interpolation heavy
def strings(d):
    for f in range(NF):
        L=[f"package st{f}", f"object S{f} {{"]
        for m in range(200):
            L.append(f"  def m{m}(a: Int, b: String, c: Double): String = s\"$a-$b-${{c * {m}}}-${{a + b.length}}\" + f\"$c%.2f $a%05d\" + raw\"\\n{m}\" + s\"${{List(a, b, c).mkString}}\"")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(st0.S0.m1(1, \"x\", 2.0))"))


# 14. implicit conversions & extension methods (RichInt, StringOps, ArrowAssoc, user views)
def extensions(d):
    for f in range(NF):
        L=[f"package ex{f}", "import scala.language.implicitConversions", "final case class Meters(v: Double)", "final case class Feet(v: Double)",
           "object Conv { implicit def feetToMeters(f: Feet): Meters = Meters(f.v * 0.3048); implicit class MetersOps(private val m: Meters) extends AnyVal { def +(o: Meters): Meters = Meters(m.v + o.v); def double: Meters = Meters(m.v * 2) } }",
           "import Conv._", f"object E{f} {{"]
        for m in range(150):
            L.append(f"  def m{m}(a: Int, s: String, f: Feet): (Meters, Int, String, Map[String, Int]) = (Meters(1) + f + (f: Meters).double, (a max {m}) min 100, s.capitalize.reverse.padTo(10, '-').toUpperCase * 2, Map(s -> a, \"k{m}\" -> (a to {m}).sum))")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(ex0.E0.m1(3, \"ab\", ex0.Feet(2)))"))

# 15. cake pattern: abstract type members, self types, path-dependent types
def cake(d):
    for f in range(NF):
        L=[f"package ck{f}",
           "trait Component { type Repr; def make(i: Int): Repr; def show(r: Repr): String }",
           "trait Storage { self: Component => private var items = List.empty[Repr]; def add(i: Int): Unit = items = make(i) :: items; def dump: String = items.map(show).mkString(\",\") }",
           "trait Logging { self: Component with Storage => def log(i: Int): String = { add(i); dump } }"]
        for c in range(40):
            L.append(f"object Cake{c} extends Component with Storage with Logging {{ type Repr = (Int, String); def make(i: Int): Repr = (i * {c}, \"c{c}\"); def show(r: Repr): String = r._2 + r._1 }}")
        L.append(f"object K{f} {{ def all: List[String] = List(" + ", ".join(f"Cake{c}.log({c})" for c in range(40)) + ") }")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(ck0.K0.all.length)"))

# 16. big sealed hierarchy with exhaustive matches
def bigenum(d):
    for f in range(6):
        L=[f"package en{f}", "sealed trait Op"]
        for c in range(300):
            L.append(f"case object O{c} extends Op")
        L.append("final case class Custom(n: Int) extends Op")
        L.append(f"object N{f} {{")
        for m in range(10):
            L.append(f"  def m{m}(o: Op): Int = o match {{")
            for c in range(300):
                L.append(f"    case O{c} => {c + m}")
            L.append("    case Custom(n) => n")
            L.append("  }")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(en0.N0.m1(en0.O5))"))

# 17. named/default args, varargs, by-name, case class copy
def args(d):
    for f in range(NF):
        L=[f"package ar{f}", "final case class Cfg(host: String = \"h\", port: Int = 80, tls: Boolean = false, tags: List[String] = Nil, retries: Int = 3, timeout: Double = 1.5)",
           f"object A{f} {{", "  def log(level: Int = 1)(msg: => String, rest: Any*): Int = if (level > 2) msg.length + rest.size else rest.size"]
        for m in range(200):
            L.append(f"  def m{m}(c: Cfg): (Cfg, Int) = (c.copy(port = c.port + {m}, tags = \"t{m}\" :: c.tags).copy(tls = true), log(level = {m%4})(s\"x{m}\", c, {m}, \"y\") + log()(\"z\"))")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(ar0.A0.m1(ar0.Cfg()))"))

# 18. nested lambdas / callbacks and Future-like combinators
def lambdas(d):
    for f in range(NF):
        L=[f"package lm{f}", "import scala.concurrent.{Future, ExecutionContext}", f"object L{f} {{"]
        for m in range(120):
            L.append(f"  def m{m}(xs: Seq[Int])(implicit ec: ExecutionContext): Future[Map[Int, List[String]]] =")
            L.append(f"    Future(xs).flatMap(a => Future.sequence(a.map(x => Future(x * {m}).map(y => (y % 5, List(y.toString)))))).map(_.groupMapReduce(_._1)(_._2)(_ ++ _)).recover {{ case _: Throwable => Map.empty }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(lm0.L0.m1(Seq(1,2,3))(scala.concurrent.ExecutionContext.global).isCompleted || true)"))

# 19. F-bounded / recursive generics and structural-ish types
def fbounded(d):
    for f in range(NF):
        L=[f"package fb{f}", "trait Ord[A <: Ord[A]] { self: A => def cmp(o: A): Int; def max(o: A): A = if (cmp(o) >= 0) self else o }",
           "final case class V(i: Int) extends Ord[V] { def cmp(o: V): Int = i - o.i }",
           "trait Builder[+Elem, +To <: Builder[Elem, To]] { def add[E >: Elem](e: E): Builder[E, To] }",
           f"object Q{f} {{", "  def best[A <: Ord[A]](xs: List[A]): A = xs.reduce(_ max _)"]
        for m in range(200):
            L.append(f"  def m{m}(xs: List[V]): (V, Option[V], Seq[V]) = (best(xs), xs.find(_.cmp(V({m})) > 0).map(_ max V({m})), xs.sortWith(_.cmp(_) < 0).take({m%5+1}))")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(fb0.Q0.m1(List(fb0.V(1), fb0.V(9))))"))


# 20. long method chains with inference
def chains(d):
    for f in range(NF):
        L=[f"package ch{f}", f"object Ch{f} {{"]
        for m in range(60):
            expr="xs"
            for k in range(25):
                op=["map(_ + %d)"%k, "filter(_ > %d)"%k, "flatMap(x => List(x, x + %d))"%k, "take(%d)"%(k+50), "distinct", "sortBy(x => -x)", "zipWithIndex.map { case (a, i) => a + i }"][k%7]
                expr+="."+op
            L.append(f"  def m{m}(xs: List[Int]): Int = {expr}.sum")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(ch0.Ch0.m1(List(1,2,3)))"))

# 21. many implicits in scope (imported object with hundreds of implicit vals/defs)
def manyimplicits(d):
    L=["package mi", "trait Codec[A] { def enc(a: A): String }", "final case class W0(i: Int)"]
    for i in range(1, 400):
        L.append(f"final case class W{i}(i: Int)")
    L.append("object Codecs {")
    for i in range(400):
        L.append(f"  implicit val c{i}: Codec[W{i}] = (a: W{i}) => \"w{i}:\" + a.i")
    L.append("  implicit def listCodec[A](implicit c: Codec[A]): Codec[List[A]] = (a: List[A]) => a.map(c.enc).mkString(\",\")")
    L.append("  implicit def optCodec[A](implicit c: Codec[A]): Codec[Option[A]] = (a: Option[A]) => a.fold(\"\")(c.enc)")
    L.append("}")
    w(d, "Base.scala", "\n".join(L)+"\n")
    for f in range(NF):
        L=[f"package mi{f}", "import mi._", "import mi.Codecs._", f"object U{f} {{", "  def enc[A](a: A)(implicit c: Codec[A]): String = c.enc(a)"]
        for m in range(150):
            i=(m*7+f)%400
            L.append(f"  def m{m}: String = enc(W{i}({m})) + enc(List(W{(i+1)%400}(1))) + enc(Option(W{(i+2)%400}(2))) + (1 -> \"x\")._2")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(mi0.U0.m1)"))

# 22. heavy overloading
def overloads(d):
    L=["package ov", "object Ov {"]
    types=["Int","Long","Double","Float","String","Boolean","Char","Short","Byte","List[Int]","Option[String]","Array[Int]","Seq[Double]","Map[String,Int]","(Int, Int)","Any"]
    for t in types:
        L.append(f"  def f(a: {t}): String = \"{t}\"")
        L.append(f"  def g(a: {t}, b: Long): String = \"{t}\"")
        L.append(f"  def h(a: Int, b: {t}): String = \"{t}2\"")
    L.append("}")
    w(d, "Base.scala", "\n".join(L)+"\n")
    lits=["1","2L","3.0","4f","\"s\"","true","'c'","(5: Short)","(6: Byte)","List(1)","Some(\"x\")","Array(1)","Seq(1.0)","Map(\"a\" -> 1)","(1, 2)","new Object"]
    for f in range(NF):
        L=[f"package ov{f}", "import ov.Ov._", f"object O{f} {{"]
        for m in range(150):
            a=lits[m%len(lits)]; b=lits[(m+3)%len(lits)]
            L.append(f"  def m{m}: String = f({a}) + g({a}, {m}L) + h({m}, {a}) + f({b}) + String.valueOf({m}) + Math.max({m}, 2) + java.util.Objects.hash(\"a\", Int.box({m}))")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(ov0.O0.m1)"))

# 23. wide case classes and tuples
def wide(d):
    for f in range(NF):
        n=30
        fields=", ".join(f"f{i}: {['Int','String','Double','Option[Int]'][i%4]}" for i in range(n))
        L=[f"package wd{f}", f"final case class Wide{f}({fields})", f"object Wd{f} {{"]
        args=", ".join(["1","\"a\"","2.0","Some(3)"][i%4] for i in range(n))
        for m in range(40):
            L.append(f"  def m{m}: (Wide{f}, Boolean, Int) = {{ val w = Wide{f}({args}); val w2 = w.copy(f0 = {m}, f4 = 7); (w2, w == w2, w2.hashCode) }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(wd0.Wd0.m1._2)"))

# 24. nested/inner classes and path-dependent types
def inner(d):
    for f in range(NF):
        L=[f"package in{f}", "class Outer(val n: Int) { class Inner(val m: Int) { def sum: Int = n + m; class Deep { def all: Int = sum * 2 } }; def mk(m: Int): Inner = new Inner(m) }", f"object In{f} {{"]
        for m in range(200):
            L.append(f"  def m{m}: Int = {{ val o = new Outer({m}); val i: o.Inner = o.mk(3); val d = new i.Deep; d.all + i.sum }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(in0.In0.m1)"))

# 25. single huge file
def hugefile(d):
    L=["package hf", "object Huge {"]
    for m in range(6000):
        L.append(f"  def m{m}(x: Int): Int = if (x > {m}) m{max(m-1,0)}(x - 1) + {m % 13} else x * {m % 7 + 1}")
    L.append("}")
    w(d, "Huge.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(hf.Huge.m10(3))"))

# 26. scala.Enumeration and Java enums/SAMs
def enums(d):
    for f in range(NF):
        L=[f"package eu{f}", f"object Color{f} extends Enumeration {{ val " + ", ".join(f"C{i}" for i in range(60)) + " = Value }", f"object E{f} {{",
           "  val r: Runnable = () => ()", "  val cmp: java.util.Comparator[String] = (a, b) => a.length - b.length"]
        for m in range(100):
            L.append(f"  def m{m}(c: Color{f}.Value): Int = c match {{ case Color{f}.C{m%60} => 1; case Color{f}.C{(m+1)%60} => 2; case _ => java.util.concurrent.TimeUnit.SECONDS.toMillis(c.id.toLong).toInt }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L)+"\n")
    w(d, "Main.scala", main_obj("", "    println(eu0.E0.m1(eu0.Color0.C3))"))


# type-level HList induction through implicit defs
def hlist(d):
    base = """package hl
sealed trait HList
final case class ::[+H, +T <: HList](head: H, tail: T) extends HList
sealed trait HNil extends HList
case object HNil extends HNil
trait Show[A] { def show(a: A): String }
object Show {
  def apply[A](implicit s: Show[A]): Show[A] = s
  implicit val int: Show[Int] = i => i.toString
  implicit val str: Show[String] = s => s
  implicit val dbl: Show[Double] = d => d.toString
  implicit val bool: Show[Boolean] = b => b.toString
  implicit def opt[A](implicit a: Show[A]): Show[Option[A]] = o => o.fold("none")(a.show)
  implicit def list[A](implicit a: Show[A]): Show[List[A]] = l => l.map(a.show).mkString("[", ",", "]")
  implicit val hnil: Show[HNil] = _ => "HNil"
  implicit def hcons[H, T <: HList](implicit h: Show[H], t: Show[T]): Show[H :: T] = l => h.show(l.head) + " :: " + t.show(l.tail)
}
"""
    w(d, "Base.scala", base)
    tys = ["Int", "String", "Double", "Boolean", "Option[Int]", "List[String]", "Option[List[Double]]"]
    vals = ["1", "\"s\"", "2.0", "true", "Some(3)", "List(\"a\")", "Some(List(1.5))"]
    for f in range(NF):
        L = [f"package hl{f}", "import hl._", f"object H{f} {{"]
        for m in range(40):
            n = 6 + (m % 10)
            t = " :: ".join(tys[(m + i) % len(tys)] for i in range(n)) + " :: HNil"
            v = "".join(f"::({vals[(m + i) % len(vals)]}, " for i in range(n)) + "HNil" + ")" * n
            L.append(f"  def m{m}: String = {{ val l: {t} = {v}; Show[{t}].show(l) }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(hl0.H0.m3)"))

# many case classes with pattern matching, copy, equality
def caseclasses(d):
    for f in range(NF):
        L = [f"package cc{f}"]
        for c in range(30):
            L.append(f"final case class P{c}(a: Int, b: String, c: Double, d: Option[Int], e: List[String], f: Boolean, g: Long, h: Char)")
        L.append(f"object C{f} {{")
        for c in range(30):
            L.append(f"  def m{c}(p: P{c}): (P{c}, Int, Boolean) = p match {{ case P{c}(a, b, _, Some(d), e :: _, true, g, h) if a > d => (p.copy(a = a + 1, b = b + e), (g + h).toInt, p == P{c}(a, b, 0.0, None, Nil, false, 0L, 'x')); case other => (other.copy(f = !other.f), other.hashCode, other.productArity > 3) }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(cc0.C0.m1(cc0.P1(3, \"b\", 1.0, Some(1), List(\"x\"), true, 2L, 'c')))"))

# default and named arguments
def defaults(d):
    for f in range(NF):
        L = [f"package df{f}", f"class Cfg(val name: String = \"n\", val size: Int = 10, val ratio: Double = 0.5, val tags: List[String] = Nil, val on: Boolean = true)",
             f"object D{f} {{",
             "  def mk(a: Int, b: String = \"b\", c: Double = 1.0, d: List[Int] = List(1, 2), e: Option[String] = None, f: Boolean = false): String = s\"$a$b$c$d$e$f\""]
        for m in range(150):
            L.append(f"  def m{m}: String = mk({m}, e = Some(\"e\"), c = {m}.5) + mk(a = {m}, f = true) + new Cfg(size = {m}, tags = List(\"t\")).name + mk({m}, \"x\", d = Nil)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(df0.D0.m1)"))

# value classes and extension methods
def valueclasses(d):
    for f in range(NF):
        L = [f"package vc{f}",
             "final class Meters(val v: Double) extends AnyVal { def +(o: Meters): Meters = new Meters(v + o.v); def *(k: Double): Meters = new Meters(v * k); override def toString = s\"${v}m\" }",
             "final class UserId(val id: Long) extends AnyVal { def next: UserId = new UserId(id + 1) }",
             "object Syntax { implicit class RichInt(private val i: Int) extends AnyVal { def meters: Meters = new Meters(i.toDouble); def squared: Int = i * i }; implicit class RichStr(private val s: String) extends AnyVal { def shout: String = s.toUpperCase + \"!\" } }",
             "import Syntax._", f"object V{f} {{"]
        for m in range(150):
            L.append(f"  def m{m}(u: UserId): (Meters, UserId, Int, String) = ({m}.meters + (3.meters * 2.0), u.next.next, ({m} + 1).squared, \"x{m}\".shout)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(vc0.V0.m1(new vc0.UserId(1)))"))

# futures and for-comprehensions with implicit ExecutionContext
def futures(d):
    for f in range(NF):
        L = [f"package fu{f}", "import scala.concurrent._", "import scala.concurrent.ExecutionContext.Implicits.global", f"object F{f} {{"]
        for m in range(100):
            L.append(f"  def m{m}(x: Int): Future[(Int, String)] = for {{ a <- Future(x + {m}); b <- Future.successful(a.toString); c <- if (a > 3) Future(a * 2) else Future.failed(new Exception(\"e\")); d <- Future.sequence(List(Future(1), Future(c))).map(_.sum).recover {{ case _: Exception => 0 }} }} yield (d, b)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(scala.concurrent.Await.result(fu0.F0.m1(5), scala.concurrent.duration.Duration.Inf))"))

# higher-kinded type classes (Functor/Monad) and syntax
def hkt(d):
    base = """package hk
trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }
trait Monad[F[_]] extends Functor[F] { def pure[A](a: A): F[A]; def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]; def map[A, B](fa: F[A])(f: A => B): F[B] = flatMap(fa)(a => pure(f(a))) }
object Monad {
  def apply[F[_]](implicit m: Monad[F]): Monad[F] = m
  implicit val optionMonad: Monad[Option] = new Monad[Option] { def pure[A](a: A) = Some(a); def flatMap[A, B](fa: Option[A])(f: A => Option[B]) = fa.flatMap(f) }
  implicit val listMonad: Monad[List] = new Monad[List] { def pure[A](a: A) = List(a); def flatMap[A, B](fa: List[A])(f: A => List[B]) = fa.flatMap(f) }
  implicit def eitherMonad[E]: Monad[({ type L[a] = Either[E, a] })#L] = new Monad[({ type L[a] = Either[E, a] })#L] { def pure[A](a: A) = Right(a); def flatMap[A, B](fa: Either[E, A])(f: A => Either[E, B]) = fa.flatMap(f) }
}
object syntax {
  implicit class MonadOps[F[_], A](private val fa: F[A]) extends AnyVal {
    def fmap[B](f: A => B)(implicit M: Monad[F]): F[B] = M.map(fa)(f)
    def bind[B](f: A => F[B])(implicit M: Monad[F]): F[B] = M.flatMap(fa)(f)
  }
}
"""
    w(d, "Base.scala", base)
    for f in range(NF):
        L = [f"package hk{f}", "import hk._", "import hk.syntax._", f"object K{f} {{",
             "  def twice[F[_]: Monad, A](fa: F[A])(f: A => A): F[A] = Monad[F].map(Monad[F].map(fa)(f))(f)"]
        for m in range(100):
            L.append(f"  def m{m}(o: Option[Int], l: List[Int]): (Option[String], List[Int], Option[Int]) = (o.fmap(_ + {m}).bind(x => Option(x.toString)), l.bind(x => List(x, x * {m})).fmap(_ + 1), twice(o)(_ * 2))")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(hk0.K0.m1(Some(1), List(1, 2)))"))

# BigInt / BigDecimal arithmetic with implicit conversions
def bignum(d):
    for f in range(NF):
        L = [f"package bn{f}", f"object B{f} {{"]
        for m in range(150):
            L.append(f"  def m{m}(x: BigInt, y: BigDecimal): (BigInt, BigDecimal, Boolean) = (x * {m} + 1 - (x pow 2) / 3 + BigInt(\"{m}\"), y * 2 + {m} / y.max(1) - BigDecimal({m}.5), x > {m} && y <= {m} * 2)")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(bn0.B0.m1(BigInt(5), BigDecimal(2)))"))

# exhaustivity over tuples of sealed hierarchies
def sealedmatch(d):
    base = """package sm
sealed trait Color; case object Red extends Color; case object Green extends Color; case object Blue extends Color
sealed trait Shape; final case class Circle(r: Double) extends Shape; final case class Rect(w: Double, h: Double) extends Shape; case object Dot extends Shape
sealed abstract class Op; object Op { case object Add extends Op; case object Sub extends Op; case object Mul extends Op; case object Div extends Op }
"""
    w(d, "Base.scala", base)
    for f in range(NF):
        L = [f"package sm{f}", "import sm._", "import sm.Op._", f"object S{f} {{"]
        for m in range(60):
            L.append(f"  def m{m}(c: Color, s: Shape, o: Op, b: Option[Color]): Int = (c, s, o, b) match {{ case (Red, Circle(r), Add, Some(Green)) => r.toInt + {m}; case (Red, _, Sub | Mul, None) => 1; case (Green, Rect(w, _), _, Some(_)) => w.toInt; case (Blue, Dot, Div, _) => 3; case (_, Circle(_), _, None) => 4; case (Green | Blue, _, Add, Some(Red)) => 5; case (_, Rect(_, h), Mul, _) => h.toInt; case _ => 0 }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(sm0.S0.m1(sm.Red, sm.Dot, sm.Op.Add, None))"))

# closures capturing vars, local defs, nested functions
def closures(d):
    for f in range(NF):
        L = [f"package cl{f}", f"object Cl{f} {{"]
        for m in range(150):
            L.append(f"  def m{m}(xs: List[Int]): Int = {{ var acc = 0; var n = {m}; def add(i: Int): Unit = {{ acc += i * n; n -= 1 }}; xs.foreach(add); val g = (y: Int) => {{ acc += y; acc }}; xs.map(g).foldLeft(0)((a, b) => a + b + acc) }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(cl0.Cl0.m1(List(1, 2, 3)))"))

# stackable trait modifications (abstract override)
def stackable(d):
    for f in range(NF):
        L = [f"package st{f}", "abstract class Base { def run(x: Int): Int; def log: List[String] = Nil }", "class Core extends Base { def run(x: Int): Int = x }"]
        for t in range(25):
            L.append(f"trait M{t} extends Base {{ abstract override def run(x: Int): Int = super.run(x + {t}) * 2 % 1000003; override def log: List[String] = \"m{t}\" :: super.log }}")
        L.append(f"object St{f} {{")
        for c in range(40):
            mix = " with ".join(f"M{(c + j) % 25}" for j in range(8))
            L.append(f"  def c{c}: Int = {{ val o = new Core with {mix}; o.run({c}) + o.log.size }}")
        L.append("}")
        w(d, f"F{f}.scala", "\n".join(L) + "\n")
    w(d, "Main.scala", main_obj("", "    println(st0.St0.c1)"))


kinds = dict(chains=chains, manyimplicits=manyimplicits, overloads=overloads, wide=wide, inner=inner, hugefile=hugefile, enums=enums, extensions=extensions, cake=cake, bigenum=bigenum, args=args, lambdas=lambdas, fbounded=fbounded, arith=arith, coll=coll, forcomp=forcomp, patmat=patmat, branches=branches, literals=literals,
             hierarchy=hierarchy, typeclass=typeclass, smallfiles=smallfiles, javainterop=javainterop,
             bigmethod=bigmethod, generics=generics, strings=strings,
             hlist=hlist, caseclasses=caseclasses, defaults=defaults, valueclasses=valueclasses, futures=futures,
             hkt=hkt, bignum=bignum, sealedmatch=sealedmatch, closures=closures, stackable=stackable)
if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = sys.argv[1]
    for k in sys.argv[2:] or kinds:
        kinds[k](os.path.join(out, k + SUFFIX))
