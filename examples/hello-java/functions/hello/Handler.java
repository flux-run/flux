// hello — Flux function (compiled to WASM via GraalVM Native Image + WASI)
// Build: native-image --no-fallback -H:Kind=SHARED_LIBRARY Handler.java
import com.oracle.svm.core.c.CTypedef;
import org.graalvm.nativeimage.c.function.CEntryPoint;
import java.nio.charset.*;

public class HelloHandler {

    @CEntryPoint(name = "hello_handler")
    public static long handle(org.graalvm.word.Pointer inputPtr, int inputLen) {
        // TODO: decode input
        byte[] resp = "{\"ok\":true}".getBytes(StandardCharsets.UTF_8);
        // Return pointer + length packed into long.
        return ((long) resp.hashCode() << 32) | resp.length;
    }
}
