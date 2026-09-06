import java.lang.instrument.Instrumentation;
import java.lang.reflect.Field;

/** Build-host workaround for Windows installations where AF_UNIX bind works but connect fails. */
public final class DisableUnixDomainPipeAgent {
    public static void premain(String arguments, Instrumentation instrumentation) throws Exception {
        Class<?> pipe = Class.forName("sun.nio.ch.PipeImpl");
        Field disabled = pipe.getDeclaredField("noUnixDomainSockets");
        disabled.setAccessible(true);
        disabled.setBoolean(null, true);
    }
}
