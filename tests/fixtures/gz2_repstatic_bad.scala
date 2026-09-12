// The wrong acceptance under gitbucket's last error, reduced: every member of
// slick's `object Rep` reached the unqualified scope of any subclass of `class
// Rep`, because scalac mirrors a companion's members onto the class file as
// `static` forwarders. scalac: "not found: value forNodeUntyped" / "not found:
// value columnPlaceholder".
abstract class Gz2RepStatic extends slick.lifted.Rep[Int] {
  def n = forNodeUntyped[Int](null)
  def p = columnPlaceholder
}
