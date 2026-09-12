// agent/libzero2, SLS 4.1: a local `def` is in scope for the whole block (which
// is what makes `sys/process/Parser.scala`'s `def cur = if (done) …` above
// `def done = …` compile), but a reference from an eager `val`'s initialiser to a
// definition that comes *later* is still a forward reference over the definition
// of a value. Real scalac 2.13.16 refuses all three.
//
// In its own file because nsc reports this from **RefChecks**, which does not
// run once the typer has reported anything -- so a fixture that also carries a
// type error would hide scalac's verdict on these.
//
// The legal neighbours (a plain statement, another `def`'s body, a `lazy val`'s
// initialiser) are in `lz2_resolve`.
object Main {
  def bad1(): Int = { val s = g; def g = 2; s }
  def bad2(): Int = { val r = { println(h); 1 }; def h = 2; r }
  def bad3(): Int = { val f = () => k; def k = 2; f() }
}
