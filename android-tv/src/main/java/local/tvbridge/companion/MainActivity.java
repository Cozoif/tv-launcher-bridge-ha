package local.tvbridge.companion;

import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ResolveInfo;
import android.graphics.Color;
import android.os.Bundle;
import android.provider.Settings;
import android.text.InputType;
import android.view.View;
import android.widget.*;
import org.json.JSONObject;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.*;

/** TV frontend for the existing wireless-ADB bridge; no privileged Android permissions. */
public class MainActivity extends Activity {
    private LinearLayout page;
    private TextView status;
    private SharedPreferences prefs;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final List<Button> networkButtons = new ArrayList<>();
    private boolean busy;
    private int generation;
    private static final int INK = Color.rgb(232, 241, 250);

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        prefs = getSharedPreferences("bridge", MODE_PRIVATE);
        home();
    }
    private int dp(int n) { return Math.round(n * getResources().getDisplayMetrics().density); }
    private void screen(String title, String subtitle) {
        generation++;
        networkButtons.clear();
        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(Color.rgb(15, 24, 40));
        page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setPadding(dp(48), dp(24), dp(48), dp(32));
        scroll.addView(page);
        setContentView(scroll);
        label(title, 30);
        label(subtitle, 17);
        status = label("", 16);
        status.setTextColor(Color.rgb(98, 219, 188));
    }
    private TextView label(String text, int size) {
        TextView view = new TextView(this);
        view.setText(text); view.setTextSize(size); view.setTextColor(INK);
        view.setPadding(0, dp(5), 0, dp(7)); page.addView(view); return view;
    }
    private Button button(String text, Runnable action) {
        Button b = new Button(this); b.setText(text); b.setAllCaps(false); b.setTextSize(18);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, dp(56));
        lp.bottomMargin = dp(6); page.addView(b, lp);
        b.setOnClickListener(v -> action.run()); return b;
    }
    private Button commandButton(String text, Runnable action) {
        Button b = button(text, action); b.setEnabled(!busy); networkButtons.add(b); return b;
    }
    private EditText field(String hint, String value, boolean password) {
        EditText e = new EditText(this); e.setHint(hint); e.setText(value); e.setSingleLine(true);
        e.setTextColor(INK); e.setHintTextColor(Color.LTGRAY); e.setTextSize(18);
        e.setInputType(password ? InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD : InputType.TYPE_CLASS_TEXT);
        page.addView(e, new LinearLayout.LayoutParams(-1, dp(58))); return e;
    }
    private void home() {
        screen("TV Bridge", "Your apps and Home Assistant, on the same home network.");
        button("Remote app launcher setup", this::receiver).requestFocus();
        button("Open an installed app", this::apps);
        button("Play a Jellyfin show", this::shows);
        commandButton("Check bridge connection", () -> request("/info", null));
        button("Bridge connection settings", this::connection);
        button("Wireless debugging setup", this::debugging);
        label("Home Assistant connects to the desktop bridge. Keep that computer running. This companion does not replace the bridge or bypass Android app permissions.", 15);
    }
    private void receiver() {
        screen("Home Assistant app launcher", "Prototype: enable the accessibility service, then test after standby with ADB disabled.");
        label(LaunchService.running ? "Receiver is listening." : "Receiver is stopped. Enable the service or switch it off and on to retry.", 17);
        String saved = prefs.getString("receiver_token", "");
        if (saved.isEmpty()) {
            byte[] bytes = new byte[24];
            new java.security.SecureRandom().nextBytes(bytes);
            StringBuilder token = new StringBuilder();
            for (byte b : bytes) token.append(String.format(java.util.Locale.ROOT, "%02x", b & 255));
            saved = token.toString();
            prefs.edit().putString("receiver_token", saved).commit();
        }
        label("Receiver port: 8130. Copy this separate secret to the HA app companion_token option.", 17);
        EditText token = field("Companion secret", saved, false);
        token.setKeyListener(null);
        EditText packages = field("Allowed package names, comma separated",
            prefs.getString("allowed_packages", "org.jellyfin.androidtv"), false);
        button("Save allowed apps", () -> {
            String allowed = packages.getText().toString().trim();
            for (String pkg : allowed.split(",")) {
                if (!pkg.trim().matches("[a-zA-Z0-9_]+(\\.[a-zA-Z0-9_]+)+")) {
                    status.setText("Enter Android package names separated by commas."); return;
                }
            }
            prefs.edit().putString("allowed_packages", allowed).apply();
            status.setText("Saved. Only these apps can be launched remotely.");
        });
        button("Enable TV Bridge accessibility service", () -> {
            try { startActivity(new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)); }
            catch (Exception e) { status.setText("Open Settings > Accessibility and enable TV Bridge."); }
        });
        label("The service receives signed commands on your home network. It does not need wireless debugging. Keep the Chromecast powered. Network sleep and force-stop can interrupt the connection.", 16);
        button("Back", this::home);
    }

    private void connection() {
        screen("Connect your bridge", "Enter the computer's address shown in the desktop app's Home Assistant tab.");
        EditText address = field("http://192.168.1.10:8125", prefs.getString("address", ""), false);
        EditText token = field("Bridge token (if configured)", prefs.getString("token", ""), true);
        button("Save and check connection", () -> {
            String base = address.getText().toString().trim();
            try {
                URI uri = new URI(base);
                if (!("http".equals(uri.getScheme()) || "https".equals(uri.getScheme())) || uri.getHost() == null || uri.getUserInfo() != null || uri.getQuery() != null || uri.getFragment() != null || !(uri.getPath().isEmpty() || "/".equals(uri.getPath()))) throw new Exception();
                prefs.edit().putString("address", base.replaceAll("/+$", "")).putString("token", token.getText().toString().trim()).apply();
                request("/info", null);
            } catch (Exception ex) { status.setText("Enter a complete address such as http://192.168.1.10:8125, without a path."); }
        });
        button("Back", this::home);
    }
    private void debugging() {
        screen("Wireless debugging", "Pair the desktop bridge once using the code displayed by this TV.");
        label("1. Enable Developer options if needed.\n2. Open Wireless debugging and choose Pair device with pairing code.\n3. In the desktop app, enter this TV's IP, pairing port and six-digit code.\n4. Use the connection port from the Wireless debugging screen, or let the desktop app discover it. The pairing port and connection port are different.", 19);
        button("Open Developer options", () -> {
            try { startActivity(new Intent(Settings.ACTION_APPLICATION_DEVELOPMENT_SETTINGS)); }
            catch (Exception e) { try { startActivity(new Intent(Settings.ACTION_SETTINGS)); } catch (Exception ignored) { status.setText("Open Settings using your TV remote."); } }
        });
        label("If this TV does not offer Wireless debugging, this APK cannot enable it. USB debugging alone is a different connection mode.", 16);
        button("Back", this::home);
    }
    private void apps() {
        screen("Installed apps", "Choose an app with your TV remote. No bridge connection is needed for this list.");
        Set<String> seen = new HashSet<>();
        List<ResolveInfo> found = new ArrayList<>();
        for (String category : new String[]{Intent.CATEGORY_LEANBACK_LAUNCHER, Intent.CATEGORY_LAUNCHER}) {
            Intent query = new Intent(Intent.ACTION_MAIN).addCategory(category);
            for (ResolveInfo r : getPackageManager().queryIntentActivities(query, 0)) {
                if (!getPackageName().equals(r.activityInfo.packageName) && seen.add(r.activityInfo.packageName)) found.add(r);
            }
        }
        found.sort((a,b) -> a.loadLabel(getPackageManager()).toString().compareToIgnoreCase(b.loadLabel(getPackageManager()).toString()));
        button("Back", this::home).requestFocus();
        for (ResolveInfo r : found) button(r.loadLabel(getPackageManager()).toString(), () -> {
            try { startActivity(new Intent(Intent.ACTION_MAIN).setClassName(r.activityInfo.packageName, r.activityInfo.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)); }
            catch (Exception e) { status.setText("This app could not be opened: " + e.getMessage()); }
        });
        if (found.isEmpty()) label("No launchable apps were found.", 18);
    }
    private void shows() {
        screen("Play a Jellyfin show", "Uses the Jellyfin server and user configured in the desktop bridge.");
        EditText show = field("Show name", "", false);
        EditText season = field("Season (optional)", "", false); season.setInputType(InputType.TYPE_CLASS_NUMBER);
        EditText episode = field("Episode (optional)", "", false); episode.setInputType(InputType.TYPE_CLASS_NUMBER);
        commandButton("Play on this TV", () -> {
            if (show.getText().toString().trim().isEmpty()) { status.setText("Enter a show name."); return; }
            try {
                JSONObject data = new JSONObject().put("show", show.getText().toString().trim());
                if (!season.getText().toString().isEmpty()) data.put("season", Integer.parseInt(season.getText().toString()));
                if (!episode.getText().toString().isEmpty()) data.put("episode", Integer.parseInt(episode.getText().toString()));
                request("/watch", data);
            } catch (Exception e) { status.setText("Season and episode must be whole numbers."); }
        });
        label("Leave season and episode blank for the next unwatched episode. Other apps can be opened here; playing a specific title depends on the app's supported commands or links.", 16);
        button("Back", this::home);
    }
    private void request(String path, JSONObject body) {
        if (busy) { status.setText("A request is already running."); return; }
        String address = prefs.getString("address", "");
        String token = prefs.getString("token", "");
        if (address.isEmpty()) { status.setText("Set the bridge connection address first."); return; }
        busy = true;
        for (Button b : networkButtons) b.setEnabled(false);
        final int current = generation;
        status.setText("Contacting bridge…");
        worker.execute(() -> {
            String result;
            HttpURLConnection conn = null;
            try {
                conn = (HttpURLConnection) new URL(address + path).openConnection();
                conn.setInstanceFollowRedirects(false);
                conn.setConnectTimeout(8000); conn.setReadTimeout(95000);
                if (!token.isEmpty()) conn.setRequestProperty("Authorization", "Bearer " + token);
                if (body != null) {
                    conn.setRequestMethod("POST"); conn.setDoOutput(true);
                    conn.setRequestProperty("Content-Type", "application/json; charset=utf-8");
                    try (OutputStream out = conn.getOutputStream()) { out.write(body.toString().getBytes(StandardCharsets.UTF_8)); }
                }
                int code = conn.getResponseCode();
                InputStream stream = code >= 400 ? conn.getErrorStream() : conn.getInputStream();
                if (stream == null) throw new IOException("Empty reply (HTTP " + code + ")");
                ByteArrayOutputStream bytes = new ByteArrayOutputStream();
                try (InputStream in = stream) {
                    byte[] buffer = new byte[4096]; int n;
                    while ((n = in.read(buffer)) != -1) {
                        if (bytes.size() + n > 1048576) throw new IOException("Bridge reply was too large");
                        bytes.write(buffer, 0, n);
                    }
                }
                JSONObject reply = new JSONObject(bytes.toString("UTF-8"));
                if (code == 401) result = "Token rejected. Check the bridge token in connection settings.";
                else if (code >= 300 || !reply.optBoolean("ok", true)) result = "Bridge: " + reply.optString("message", "Request failed (HTTP " + code + ")");
                else result = path.equals("/info") ? "Connected to " + reply.optString("name", "TV Launcher Bridge") + ". Home Assistant uses this same bridge address." : reply.optString("message", "Command accepted");
            } catch (Exception e) { result = "Could not complete request: " + e.getMessage(); }
            finally { if (conn != null) conn.disconnect(); }
            final String message = result;
            runOnUiThread(() -> {
                busy = false;
                if (isDestroyed()) return;
                for (Button b : networkButtons) b.setEnabled(true);
                if (current == generation) status.setText(message);
            });
        });
    }
    @Override protected void onDestroy() { worker.shutdownNow(); super.onDestroy(); }
}
