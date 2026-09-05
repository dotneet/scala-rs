// Passing a class **this run is compiling** to a macro's type tag
// (`docs/macros.md` §5.1). This is gitbucket's shape: every one of its 35
// `lazy val Issues = TableQuery[Issues]` has a table class declared beside the
// call and no class file anywhere.
//
// The engine's mirror resolves a class by name against the macro classpath, so
// it can never find one of these. It is handed a *placeholder* symbol instead
// -- the class's full name and no info -- and scala-rs recognises that name in
// the tree that comes back and puts its own type there. `MgLocal` below is the
// top-level case; `MgNest.Row` is a class nested in a trait, which is the
// shape gitbucket actually writes (`class Issues(tag: Tag)` inside
// `trait IssueComponent`) and which has no `staticClass` path even once it is
// compiled.
//
// Compiled by scala-rs against `mg_lib.scala`'s class files, which real scalac
// wrote, and run. A separate test compiles this same file with real scalac and
// pins that the two programs print the same thing: a macro that expands to a
// *different* tree still compiles, so only the output can say the expansion
// was right.
import mgl.api._
import mgl.{MgCase, MgName}

class MgLocal(tag: MgTag) {
  def label: String = "local" + tag.n
}

trait MgNest {
  // Declared before the class it mentions, exactly as gitbucket writes it.
  lazy val rows = MgQuery[Row]

  class Row(tag: MgTag) {
    def label: String = "nested" + tag.n
  }
}

object Main extends MgNest {
  lazy val locals = MgQuery[MgLocal]
  // A macro that *inspects* its type argument still gets the real symbol when
  // the class is on the macro classpath: nothing here is a placeholder.
  val named = MgName.of[MgCase]

  def main(args: Array[String]): Unit = {
    println(locals.at(1).label)
    println(locals.at(7).label)
    println(rows.at(2).label)
    // The macro's result is a real `MgQuery[MgLocal]`, so its members resolve.
    val q: MgQuery[MgLocal] = locals
    println(q.at(3).label)
    println(named)
  }
}
