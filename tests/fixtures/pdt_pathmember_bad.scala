// Keeping the prefix is not a licence to accept anything, and dropping it is
// not a licence either. nsc 2.13.16 rejects all three of these.
//
//  1. `p.T` and `q.T` are different types: what one instance's `get` returns
//     is not what another instance's `put` takes.
//  2. The same, spelled as a written type rather than inferred.
//  3. A name the path's class does not declare is still "not a member".

trait Box {
  type T
  def get: T
  def put(t: T): Unit
}

object Main {
  def swap(p: Box, q: Box): Unit = q.put(p.get)

  def annotated(p: Box, q: Box): Unit = {
    val x: p.T = q.get
    println(x)
  }

  def missing(p: Box): Unit = {
    val y: p.Nope = null.asInstanceOf[p.Nope]
    println(y)
  }

  def main(args: Array[String]): Unit = println("unreachable")
}
