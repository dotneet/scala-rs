// Two parameter types that really are the *same* type are still an override
// and still need the modifier: only a genuinely different parameter class
// makes cats' `compose` tower an overload. scalac 2.13.16 rejects this too
// ("needs `override' modifier"), which it can only do in a file with no type
// error -- its override check runs in `refchecks`, after `typer`.

trait Base[F[_]] {
  def compose[G[_]](implicit ev: Base[G]): String = "base"
}
trait Derived[F[_]] extends Base[F] {
  def compose[G[_]](implicit ev: Base[G]): String = "derived"
}
