#!/usr/bin/env python3
"""Generate the macro-heavy circe/shapeless workload of docs/performance.md.

Each of FILES sources declares PER case classes, each deriving an
`Encoder` and a `Decoder` with circe's `semiauto` (a shapeless `Lazy` /
`LabelledGeneric` derivation per instance, the later classes holding the
earlier ones), a sealed trait with four cases derived the same way, and an
object whose values are derived by `io.circe.generic.auto`. `Main` prints
every round trip, so two compilers' outputs can be compared by running it.

    tests/macro_bench_gen.py OUT [--files 30] [--per 10] [--no-coproduct-decoder]

Classpath: circe-core/generic/parser/jawn/numbers 0.14.7, cats-core and
cats-kernel 2.11.0, jawn-parser 1.5.1, shapeless 2.3.13, and scala-reflect
plus scala-compiler 2.13.16 (shapeless's `Lazy` needs the compiler's
classes to expand). `--no-coproduct-decoder` leaves out the sealed traits'
decoders, which scala-rs could not compile before 2026-09-26, so that older
builds can be measured on the same sources.
"""

import argparse
import os


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("out")
    parser.add_argument("--files", type=int, default=30)
    parser.add_argument("--per", type=int, default=10)
    parser.add_argument("--no-coproduct-decoder", action="store_true")
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    for f in range(args.files):
        lines = [f"package bench.p{f}", "", "import io.circe._", "import io.circe.generic.semiauto._", ""]
        for c in range(args.per):
            name = f"C{f}_{c}"
            fields = ["id: Int", "name: String", "tags: List[String]", "score: Option[Double]",
                      "attrs: Map[String, Int]"]
            if c > 0:
                prev = f"C{f}_{c - 1}"
                fields += [f"child: {prev}", f"children: Vector[{prev}]"]
            lines += [
                f"final case class {name}({', '.join(fields)})",
                f"object {name} {{",
                f"  implicit val enc: Encoder[{name}] = deriveEncoder[{name}]",
                f"  implicit val dec: Decoder[{name}] = deriveDecoder[{name}]",
                "}",
            ]
        shape = f"Shape{f}"
        lines += [f"sealed trait {shape}", f"object {shape} {{"]
        for k in range(4):
            lines.append(f"  final case class V{k}(a: Int, b: String, c: C{f}_0) extends {shape}")
        lines.append(f"  implicit val enc: Encoder[{shape}] = deriveEncoder[{shape}]")
        if not args.no_coproduct_decoder:
            lines.append(f"  implicit val dec: Decoder[{shape}] = deriveDecoder[{shape}]")
        lines += [
            "}",
            f"object Auto{f} {{",
            "  import io.circe.generic.auto._",
            "  import io.circe.syntax._",
            "  final case class A(x: Int, y: List[String], z: Option[B])",
            f"  final case class B(p: Double, q: Map[String, List[Int]], r: C{f}_0)",
            f"  def run(): String = A(1, List(\"a\"), Some(B(2.0, Map(\"k\" -> List(1)), "
            f"C{f}_0(1, \"n\", Nil, None, Map.empty)))).asJson.noSpaces",
            "  def back(s: String): Either[Error, A] = parser.decode[A](s)",
            "}",
        ]
        with open(os.path.join(args.out, f"F{f}.scala"), "w") as out:
            out.write("\n".join(lines) + "\n")
    main_lines = ["package bench", "import io.circe.syntax._", "object Main {",
                  "  def main(args: Array[String]): Unit = {"]
    for f in range(args.files):
        main_lines.append(f"    val s{f} = p{f}.Auto{f}.run(); println(s{f}); println(p{f}.Auto{f}.back(s{f}).isRight)")
        main_lines.append(f"    val v{f}: p{f}.Shape{f} = p{f}.Shape{f}.V1(1, \"x\", "
                          f"p{f}.C{f}_0(1, \"n\", Nil, None, Map.empty)); println(v{f}.asJson.noSpaces)")
    main_lines += ["  }", "}"]
    with open(os.path.join(args.out, "Main.scala"), "w") as out:
        out.write("\n".join(main_lines) + "\n")


if __name__ == "__main__":
    main()
