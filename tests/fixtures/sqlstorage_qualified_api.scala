package sqlstoragequalified
class Body {
 private[sqlstoragequalified] val value: Int = 14
 private[sqlstoragequalified] var count: Int = 1
}
class Constructor(private[sqlstoragequalified] val value: Int,
                  private[sqlstoragequalified] var count: Int)
