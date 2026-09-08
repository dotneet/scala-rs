// The Java half of the `agent/javavarargs` fixtures. Compiled by javac and
// read back as a class file, which is the only way this compiler sees Java --
// and the same arrangement `tests/scalalib_measure.sh` uses for the library's
// own Java sources.
//
// `pick` is the shape the defect was found on: `java.lang.reflect.Array`
// declares `newInstance(Class<?>, int)` beside `newInstance(Class<?>, int...)`
// and twelve standard-library calls were `ambiguous overload` between them.
// Every method here reports which alternative ran, so a wrong pick is a wrong
// line of output rather than a program that merely compiles.
package jvarargs;

public class JVar {
  // A fixed-arity alternative beside its own varargs sibling.
  public static String pick(int x) { return "java-fixed(" + x + ")"; }
  public static String pick(int... xs) { return "java-varargs(" + xs.length + ")"; }

  // Reachable only through the varargs alternative: a primitive element type,
  // so the array the call builds must be an `int[]` and not an `Object[]`.
  public static String only(int... xs) {
    int sum = 0;
    for (int x : xs) sum += x;
    return "java-only(" + xs.length + "," + sum + ")";
  }

  // A reference element type, and a widening one.
  public static String refs(String... xs) { return "java-refs(" + xs.length + ")"; }
  public static String wide(long... xs) { return "java-wide(" + xs.length + "," + xs[0] + ")"; }

  // An instance method whose fixed-arity formal is a *supertype* of the
  // sequence the varargs one stands for. scalac 2.13.16 still takes the
  // fixed-arity alternative here.
  public String inst(Object o) { return "java-inst-fixed"; }
  public String inst(Object... o) { return "java-inst-varargs(" + o.length + ")"; }
}
