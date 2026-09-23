package compiledqueryorder

trait ShapeLevel
trait NestedShapeLevel extends ShapeLevel
trait FlatShapeLevel extends NestedShapeLevel
trait ColumnsShapeLevel extends FlatShapeLevel

abstract class Shape[Level <: ShapeLevel, -Mixed_, Unpacked_, Packed_] {
  type Unpacked = Unpacked_
  type Packed = Packed_
}
