// The package-object aliases a client names the newtypes by, in their own
// compilation unit: `cats.data`'s `type NonEmptySet[A] = NonEmptySetImpl.Type[A]`
// is pickled separately from `NonEmptySetImpl`, and the two have to agree about
// what `Type` is.
package object rhfa {
  type NSet[A] = rhf.SetImpl.Type[A]
  type NChain[+A] = rhf.ChainImpl.Type[A]
}
