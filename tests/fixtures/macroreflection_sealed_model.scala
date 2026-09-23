package sealedmodel

sealed trait Shape

object Shape {
  final case class Circle(radius: Int) extends Shape
  case object Empty extends Shape
}
