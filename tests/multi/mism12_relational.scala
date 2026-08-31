// The middle of the cake, deliberately after the leaf on the command line.
package mism12.relational

import mism12.basic.BasicProfile

trait RelationalProfile extends BasicProfile { self: RelationalProfile =>
  def describe(sd: SchemaDescription): String = sd.show
}
