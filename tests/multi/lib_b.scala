package demo.ast

import demo.util.Box

trait Named { def name: String }

case class Node(name: String, box: Box) extends Named
