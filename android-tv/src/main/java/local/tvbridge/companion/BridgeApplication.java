package local.tvbridge.companion;

import android.app.Application;

/** Persist crash class and code locations only. Never include request bodies or secret values. */
public class BridgeApplication extends Application {
    @Override public void onCreate() {
        super.onCreate();
        Thread.UncaughtExceptionHandler previous = Thread.getDefaultUncaughtExceptionHandler();
        Thread.setDefaultUncaughtExceptionHandler((thread, error) -> {
            StringBuilder detail = new StringBuilder(error.getClass().getSimpleName());
            int count = 0;
            for (StackTraceElement frame : error.getStackTrace()) {
                if (frame.getClassName().startsWith("local.tvbridge.") && count++ < 4)
                    detail.append(" at ").append(frame.getClassName()).append(".")
                        .append(frame.getMethodName()).append(":").append(frame.getLineNumber());
            }
            getSharedPreferences("bridge", MODE_PRIVATE).edit()
                .putString("last_crash", detail.toString()).commit();
            if (previous != null) previous.uncaughtException(thread, error);
            else {
                android.os.Process.killProcess(android.os.Process.myPid());
                System.exit(1);
            }
        });
    }
}
