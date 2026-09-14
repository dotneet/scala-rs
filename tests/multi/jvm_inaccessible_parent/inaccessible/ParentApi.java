package inaccessible;

public interface ParentApi {
    default int inherited() {
        return 11;
    }

    String toString();
}
