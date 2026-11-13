package local.tvbridge.companion;

import android.accessibilityservice.AccessibilityService;
import android.content.Intent;
import android.os.Handler;
import android.os.Looper;
import android.view.accessibility.AccessibilityEvent;
import org.json.JSONObject;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;

/** Explicitly enabled local launcher. Never reads screen text or injects shell commands. */
public class LaunchService extends AccessibilityService {
    public static volatile boolean running;
    private volatile ServerSocket listener;
    private boolean starting;
    private volatile boolean closing;
    private volatile String foreground = "";
    private final ExecutorService clients = new ThreadPoolExecutor(
        1, 2, 30, TimeUnit.SECONDS, new ArrayBlockingQueue<>(4),
        new ThreadPoolExecutor.AbortPolicy());
    private final Map<String, Long> nonces = new HashMap<>();
    private final Handler main = new Handler(Looper.getMainLooper());

    @Override protected synchronized void onServiceConnected() {
        super.onServiceConnected();
        if (starting || running) return;
        closing = false;
        starting = true;
        record("Service connected", "");
        new Thread(() -> {
            try (ServerSocket socket = new ServerSocket()) {
                socket.setReuseAddress(true);
                socket.bind(new InetSocketAddress(8130));
                listener = socket;
                running = true;
                record("Receiver listening on port 8130", "");
                while (!closing) {
                    Socket client = socket.accept();
                    try { clients.execute(() -> serve(client)); }
                    catch (RejectedExecutionException e) { client.close(); }
                }
            } catch (Exception error) {
                if (!closing) record("Receiver failed", error.getClass().getSimpleName());
            } finally {
                listener = null;
                running = false;
                synchronized (LaunchService.this) { starting = false; }
            }
        }, "companion-listener").start();
    }

    @Override public void onAccessibilityEvent(AccessibilityEvent event) {
        if (event.getEventType() == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED
                && event.getPackageName() != null) {
            foreground = event.getPackageName().toString();
        }
    }
    private void record(String event, String error) {
        getSharedPreferences("bridge", MODE_PRIVATE).edit()
            .putString("service_event", event)
            .putLong("service_event_time", System.currentTimeMillis())
            .putString("service_error", error).apply();
    }

    @Override public void onInterrupt() {
        record("Android interrupted service feedback", "");
    }

    @Override public boolean onUnbind(Intent intent) {
        record("Android unbound accessibility service", "");
        return super.onUnbind(intent);
    }
    @Override public void onDestroy() {
        record("Android destroyed accessibility service", "");
        running = false;
        closing = true;
        try { if (listener != null) listener.close(); } catch (IOException ignored) {}
        clients.shutdownNow();
        super.onDestroy();
    }

    private static String line(InputStream in) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        int c;
        while ((c = in.read()) != -1) {
            if (c == '\n') return out.toString("US-ASCII").replace("\r", "");
            if (out.size() >= 2048) throw new IOException("Header too long");
            out.write(c);
        }
        throw new EOFException();
    }

    private void serve(Socket client) {
        try (Socket socket = client) {
            socket.setSoTimeout(5000);
            InputStream in = socket.getInputStream();
            String[] request = line(in).split(" ");
            if (request.length != 3) return;
            Map<String, String> headers = new HashMap<>();
            for (int count = 0; ; count++) {
                String h = line(in);
                if (h.isEmpty()) break;
                if (count >= 20 || !h.contains(":")) return;
                String key = h.substring(0, h.indexOf(':')).toLowerCase(Locale.ROOT);
                if (headers.put(key, h.substring(h.indexOf(':') + 1).trim()) != null) return;
            }
            int size = Integer.parseInt(headers.getOrDefault("content-length", "0"));
            if (size < 0 || size > 2048 || headers.containsKey("transfer-encoding")) return;
            byte[] body = new byte[size];
            new DataInputStream(in).readFully(body);
            String text = new String(body, StandardCharsets.UTF_8);
            String stamp = headers.getOrDefault("x-tv-time", "");
            String nonce = headers.getOrDefault("x-tv-nonce", "");
            String key = getSharedPreferences("bridge", MODE_PRIVATE).getString("receiver_token", "");
            long now = System.currentTimeMillis() / 1000;
            if (key.length() < 32 || nonce.length() != 32
                    || Math.abs(now - Long.parseLong(stamp)) > 60) {
                reply(socket, 401, new JSONObject().put("ok", false)); return;
            }
            String payload = request[0] + "\n" + request[1] + "\n" + stamp + "\n" + nonce + "\n" + text;
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(key.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
            StringBuilder expected = new StringBuilder();
            for (byte b : mac.doFinal(payload.getBytes(StandardCharsets.UTF_8)))
                expected.append(String.format(Locale.ROOT, "%02x", b & 255));
            if (!MessageDigest.isEqual(expected.toString().getBytes(StandardCharsets.US_ASCII),
                    headers.getOrDefault("x-tv-signature", "").getBytes(StandardCharsets.US_ASCII))) {
                reply(socket, 401, new JSONObject().put("ok", false)); return;
            }
            synchronized (nonces) {
                nonces.entrySet().removeIf(e -> e.getValue() < now - 60);
                if (nonces.containsKey(nonce) || nonces.size() >= 512) {
                    reply(socket, 409, new JSONObject().put("ok", false)); return;
                }
                nonces.put(nonce, now);
            }
            if ("GET".equals(request[0]) && "/status".equals(request[1])) {
                reply(socket, 200, new JSONObject().put("ok", true).put("current_package", foreground)
                    .put("awake", ((android.os.PowerManager) getSystemService(POWER_SERVICE)).isInteractive())
                    .put("model", android.os.Build.MODEL));
                return;
            }
            if (!"POST".equals(request[0]) || !"/launch".equals(request[1])) {
                reply(socket, 404, new JSONObject().put("ok", false)); return;
            }
            String pkg = new JSONObject(text).getString("package");
            String allowed = getSharedPreferences("bridge", MODE_PRIVATE)
                .getString("allowed_packages", "org.jellyfin.androidtv");
            boolean permitted = Arrays.stream(allowed.split(",")).anyMatch(p -> p.trim().equals(pkg));
            if (!permitted) {
                reply(socket, 403, new JSONObject().put("ok", false).put("message", "App not allowed")); return;
            }
            FutureTask<Boolean> launch = new FutureTask<>(() -> {
                Intent intent = getPackageManager().getLeanbackLaunchIntentForPackage(pkg);
                if (intent == null) intent = getPackageManager().getLaunchIntentForPackage(pkg);
                if (intent == null) return false;
                startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                return true;
            });
            main.post(launch);
            boolean accepted = launch.get(4, TimeUnit.SECONDS);
            reply(socket, accepted ? 200 : 404, new JSONObject().put("ok", accepted)
                .put("message", accepted ? "Launch requested" : "App not installed"));
        } catch (Exception ignored) {}
    }

    private static void reply(Socket socket, int code, JSONObject result) throws IOException {
        byte[] body = result.toString().getBytes(StandardCharsets.UTF_8);
        OutputStream out = socket.getOutputStream();
        out.write(("HTTP/1.1 " + code + " Result\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: "
            + body.length + "\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
        out.write(body);
        out.flush();
    }
}
