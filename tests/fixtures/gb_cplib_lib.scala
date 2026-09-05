// Compiled on its own; `gb_cplib_main.scala` sees it only as class files.
package gbcp

trait ValueType[T] { def make(): T }

class MappingValueType[T](mk: () => T) extends ValueType[T] {
  def make(): T = mk()
}

object Forms {
  // The consumer never writes `MappingValueType` -- it only ever *infers* it
  // from this result type, which is what stopped the class file from being
  // read at all.
  def mapping[T](mk: () => T): MappingValueType[T] = new MappingValueType[T](mk)
}
