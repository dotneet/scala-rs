// The class-file half of `-Xsource-features:case-apply-copy-access`. Nothing
// here uses `apply` or `copy`, so no reference forces the widening that both
// nsc (`makeNotPrivate`, which also renames) and this compiler apply to a
// `private` member read from another class file -- what `javap -p` shows is
// the access the feature put there, and it matches scalac 2.13.16 exactly:
//
//   flag off                                    flag on
//   public final class C$ extends               public final class C$
//     scala.runtime.AbstractFunction1             implements java.io.Serializable
//   public C apply(int)                         private C apply(int)
//   public C copy(int)                          private C copy(int)
//
// `private[xflags]` keeps a public method (the qualifier is erased) but still
// costs the companion its `FunctionN` parent, and `protected` reaches `copy`
// only -- nsc's `Unapplies.applyAccess` ignores it for `apply`.
package xflags

case class C private (x: Int)
case class D protected (x: Int)
case class E private[xflags] (x: Int)
case class F(x: Int)
