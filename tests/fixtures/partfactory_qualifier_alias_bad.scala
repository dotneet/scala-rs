class Test {
  trait St[T] { def map[U](f: T => U): St[U] }
  trait Sq[+T] { def toSt[B >: T]: St[B] }
  trait El
  type Alias = El
def f(ts: Sq[Alias]): St[Alias] = ts.toSt.map(x => x)
}
