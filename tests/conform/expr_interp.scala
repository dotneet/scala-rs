// An interpreter loop: nested pattern matches over a sealed tree with a mutable
// environment, recursion, and Ordering/sorting of derived keys.
object Main {
  sealed trait Expr
  case class Num(n: Int) extends Expr
  case class Var(s: String) extends Expr
  case class Add(l: Expr, r: Expr) extends Expr
  case class Mul(l: Expr, r: Expr) extends Expr
  case class Let(n: String, v: Expr, body: Expr) extends Expr
  case class If(c: Expr, t: Expr, e: Expr) extends Expr

  def eval(e: Expr, env: Map[String, Int]): Int = e match {
    case Num(n) => n
    case Var(s) => env.getOrElse(s, throw new NoSuchElementException(s))
    case Add(Num(0), r) => eval(r, env)
    case Add(l, Num(0)) => eval(l, env)
    case Add(l, r) => eval(l, env) + eval(r, env)
    case Mul(Num(1), r) => eval(r, env)
    case Mul(l, r) => eval(l, env) * eval(r, env)
    case Let(n, v, b) => eval(b, env.updated(n, eval(v, env)))
    case If(c, t, f) => if (eval(c, env) != 0) eval(t, env) else eval(f, env)
  }

  def vars(e: Expr): Set[String] = e match {
    case Var(s) => Set(s)
    case Add(l, r) => vars(l) ++ vars(r)
    case Mul(l, r) => vars(l) ++ vars(r)
    case Let(n, v, b) => vars(v) ++ (vars(b) - n)
    case If(c, t, f) => vars(c) ++ vars(t) ++ vars(f)
    case _ => Set.empty
  }

  def show(e: Expr): String = e match {
    case Num(n) => n.toString
    case Var(s) => s
    case Add(l, r) => s"(${show(l)} + ${show(r)})"
    case Mul(l, r) => s"(${show(l)} * ${show(r)})"
    case Let(n, v, b) => s"let $n = ${show(v)} in ${show(b)}"
    case If(c, t, f) => s"if ${show(c)} then ${show(t)} else ${show(f)}"
  }

  def depth(e: Expr): Int = e match {
    case Add(l, r) => 1 + math.max(depth(l), depth(r))
    case Mul(l, r) => 1 + math.max(depth(l), depth(r))
    case Let(_, v, b) => 1 + math.max(depth(v), depth(b))
    case If(c, t, f) => 1 + List(depth(c), depth(t), depth(f)).max
    case _ => 1
  }

  def main(args: Array[String]): Unit = {
    val e = Let("x", Num(3), Add(Mul(Var("x"), Num(4)), If(Var("x"), Num(10), Var("y"))))
    println(show(e))
    println(eval(e, Map.empty))
    println(vars(e).toList.sorted)
    println(depth(e))

    val es = List(Num(1), Add(Num(0), Num(5)), Mul(Num(1), Var("z")), Let("a", Num(2), Var("a")))
    println(es.map(show))
    println(es.map(x => scala.util.Try(eval(x, Map("z" -> 7))).toOption))
    println(es.sortBy(x => (depth(x), show(x))).map(show))
    println(es.groupBy(depth).toSeq.sortBy(_._1).map { case (d, l) => d -> l.size })
    println(es.map(vars).reduce(_ ++ _).toList.sorted)

    var evals = 0
    def counted(x: Expr): Int = { evals += 1; eval(x, Map("z" -> 1)) }
    println(es.map(counted).sum)
    println(evals)
    println(try eval(Var("nope"), Map.empty) catch { case e: NoSuchElementException => "unbound " + e.getMessage })
  }
}
