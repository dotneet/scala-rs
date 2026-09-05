// A class that arrives on `-cp` and is never *named* by this source: its type
// only ever comes back from a signature. Nothing had forced its class file, so
// its parent list was still the empty `AnyRef` and `takes` -- whose parameter
// is the parent -- was applicable to nothing.
//
// This is scalatra-forms' `mapping(...): MappingValueType[T]` handed to
// gitbucket's `post[T](path: String, form: ValueType[T])(action: T => Any)`.
// See docs/gitbucket.md.

import gbcp.Forms

class Form(val branch: String)

object Main {
  val uploadForm = Forms.mapping(() => new Form("b"))

  def takes[T](path: String, form: gbcp.ValueType[T])(action: T => Any): Any =
    action(form.make())

  def main(args: Array[String]): Unit = {
    println(takes("/x", uploadForm) { f => "got:" + f.branch })
  }
}
