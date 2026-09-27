import java.nio.file.Files;
import java.nio.file.Path;

public final class BatchCompile {
    public static void main(String[] args) throws Exception {
        if (args.length != 3 && args.length != 5) {
            throw new IllegalArgumentException(
                "Usage: BatchCompile source classpath repeats [native-binary scala-library]");
        }
        Path source = Path.of(args[0]);
        String classpath = args[1];
        int repeats = Integer.parseInt(args[2]);
        boolean nativeMode = args.length == 5;
        Path root = Files.createTempDirectory("compiler-batch-");
        long total = 0;
        for (int i = 0; i < repeats; i++) {
            Path input = Files.copy(source, root.resolve("input-" + i + ".scala"));
            Path output = Files.createDirectory(root.resolve("out-" + i));
            long start = System.nanoTime();
            boolean success;
            if (nativeMode) {
                Process process = new ProcessBuilder(args[3], "compile", input.toString(),
                    "-cp", classpath, "--scala-library", args[4], "-nowarn", "-d", output.toString())
                    .redirectErrorStream(true)
                    .redirectOutput(root.resolve("native-" + i + ".log").toFile())
                    .start();
                success = process.waitFor() == 0;
            } else {
                success = new scala.tools.nsc.MainClass().process(
                    new String[] {"-nowarn", "-cp", classpath, "-d", output.toString(), input.toString()});
            }
            long elapsed = System.nanoTime() - start;
            if (!success) {
                throw new IllegalStateException("Compilation failed at iteration " + i + " in " + root);
            }
            total += elapsed;
            System.out.printf("%d %.3f%n", i, elapsed / 1e9);
        }
        System.out.printf("total %.3f%n", total / 1e9);
        System.out.println(root);
    }
}
