// The *nested* spelling. SLS 9.2: a `package` clause opens the package it
// names, so `package top { package inner { … } }` opens both and `top`'s own
// `Helper` is in scope unqualified. Written `package top.inner` it would not
// be -- nsc 2.13.16 reports `not found: value Helper` for that, with and
// without `-Xsource:3` -- and this file used to be written that way, passing
// only because of the too-loose package walk `agent/proj` left behind
// (`expose_from_unopened_packages`, deleted in `agent/tail6`). The qualified
// spelling is pinned down by `crates/cli/tests/proj.rs`.
package top {
  package inner {
    object Main { def main(a: Array[String]): Unit = println(Helper.v) }
  }
}
