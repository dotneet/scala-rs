// A dotted package clause opens only the package it names: `Top` is a
// member of `p`, which `package p.q.r` does not open (scalac 2.13.16:
// "not found: type Top").
package p { class Top }
package p.q.r {
  object Use {
    def top = new Top
  }
}
