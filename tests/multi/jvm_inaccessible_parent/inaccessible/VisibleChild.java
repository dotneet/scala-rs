package inaccessible;

public class VisibleChild extends HiddenBase implements ChildApi {
    @Override
    public String toString() {
        return "visible";
    }
}
