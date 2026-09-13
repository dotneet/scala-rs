// Its descriptor is scanned before the Scala source is typed. That used to
// leave AbstractMethodError as a JAVA stub with only AnyRef as its parent.
public final class PatternErrorHolder {
  public static AbstractMethodError identity(AbstractMethodError error) {
    return error;
  }
}
