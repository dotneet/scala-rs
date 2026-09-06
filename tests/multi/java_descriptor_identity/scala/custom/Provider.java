package scala.custom;
public class Provider {
  public static List value = new List();
  public static List make() { return new List(); }
  public static String string = new String();
  public static FunctionThing function = new FunctionThing();
  public static java.lang.String accept(List value) { return value.label(); }
}
