// neg/abstract-class-2: `def f(x: S1)` written in `P2` is `f(x: P2.this.S1)`
// and does not implement `def f(x: p.S1)` -- "their prefixes (i.e., enclosing
// instances) differ". nsc reports this from refchecks, so it lives in its own
// fixture: a typer error elsewhere in the file would hide it.
class P {
  trait S1
  val p = new P
  trait S2 { def f(x: p.S1): Int }
}
class P2 extends P {
  object O2 extends S2 { def f(x: S1) = 5 }                  // error: object creation impossible
}
