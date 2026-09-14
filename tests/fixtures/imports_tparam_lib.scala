package imported

trait ExportParent {
  def noisy[A, B](a: A, b: B): (A, B) = (a, b)
}

object Exports extends ExportParent
