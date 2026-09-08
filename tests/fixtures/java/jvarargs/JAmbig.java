// The Java ties scalac 2.13.16 refuses to break. `agent/javavarargs` gives a
// fixed-arity alternative the win over its own varargs sibling; these two say
// how far that goes, and they are the reason the rule is nsc's `isAsSpecific`
// and not "a fixed-arity alternative wins".
package jvarargs;

public class JAmbig {
  // Neither is as specific as the other: `Object` does not conform to `int`,
  // and `int...` conforms to no ordinary formal.
  public static String b(Object x) { return "b-object"; }
  public static String b(int... xs) { return "b-varargs"; }

  // Two varargs lists. Read as argument types both repeated parameters are
  // unwrapped, and then each signature accepts the other's.
  public static String d(int x, int... xs) { return "d-one-plus"; }
  public static String d(int... xs) { return "d-all"; }
}
