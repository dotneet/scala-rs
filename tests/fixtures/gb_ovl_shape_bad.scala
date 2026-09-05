// The shape filter must not accept a literal whose arity fits no alternative,
// and an error *inside* a literal must not cost the literal its parameter
// types. Real scalac 2.13.16 rejects both of these too (with `not found:
// value oopsUndefined` and `overloaded method only ... does not match
// arguments ((?, ?, ?) => ?)`); the shape filter only ever narrows a set that
// is already ambiguous, so the second is reported here as an ambiguity
// rather than as "no matching overload".

class Repo(val nm: String)
class Form(val branch: String)
class VT[T]
class MVT[T] extends VT[T]

class Auth {
  def only(action: Repo => Any): String = "1"
  def only[T](action: (T, Repo) => Any): T => String = (form: T) => "2"
}

class Ctl extends Auth {
  def post[T](path: String, form: VT[T])(action: T => Any): Unit = ()
  val uploadForm: MVT[Form] = new MVT[Form]

  // One error -- `oopsUndefined` -- and one only. `form` keeps the type the
  // parameter gave it, so `form.branch` resolves; re-typing the literal with
  // no expected type would make it `Any` and add an error per field read.
  post("/x", uploadForm)(only { (form, repo) =>
    oopsUndefined(form.branch) + repo.nm
  })

  // Three parameters fit neither alternative's arity.
  val bad = only { (a, b, c) => a }
}
