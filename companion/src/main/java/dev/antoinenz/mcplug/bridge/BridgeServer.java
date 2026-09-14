package dev.antoinenz.mcplug.bridge;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.title.Title;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;

/** Localhost HTTP listener mcplug talks to. Plain JDK HttpServer: no dependencies. */
final class BridgeServer {
    private final McplugBridge plugin;
    private final int port;
    private final String token;
    private HttpServer http;

    BridgeServer(McplugBridge plugin, int port, String token) {
        this.plugin = plugin;
        this.port = port;
        this.token = token;
    }

    void start() throws IOException {
        http = HttpServer.create(new InetSocketAddress("127.0.0.1", port), 8);
        http.createContext("/status", ex -> handle(ex, "GET", this::status));
        http.createContext("/broadcast", ex -> handle(ex, "POST", this::broadcast));
        http.createContext("/countdown", ex -> handle(ex, "POST", this::countdown));
        http.createContext("/notify", ex -> handle(ex, "POST", this::notifyOps));
        http.setExecutor(null);
        http.start();
    }

    void stop() {
        http.stop(0);
    }

    private interface Handler {
        String run(String body) throws Exception;
    }

    private void handle(HttpExchange ex, String method, Handler h) throws IOException {
        try {
            if (!method.equals(ex.getRequestMethod())) {
                respond(ex, 405, "{\"error\":\"method\"}");
                return;
            }
            String auth = ex.getRequestHeaders().getFirst("Authorization");
            if (auth == null || !auth.equals("Bearer " + token)) {
                respond(ex, 401, "{\"error\":\"unauthorized\"}");
                return;
            }
            String body;
            try (InputStream in = ex.getRequestBody()) {
                body = new String(in.readAllBytes(), StandardCharsets.UTF_8);
            }
            respond(ex, 200, h.run(body));
        } catch (Exception e) {
            respond(ex, 500, "{\"error\":" + Json.str(e.getMessage()) + "}");
        }
    }

    private static void respond(HttpExchange ex, int code, String json) throws IOException {
        byte[] b = json.getBytes(StandardCharsets.UTF_8);
        ex.getResponseHeaders().set("Content-Type", "application/json");
        ex.sendResponseHeaders(code, b.length);
        try (OutputStream out = ex.getResponseBody()) {
            out.write(b);
        }
    }

    /** Runs on the server thread and waits for the result (the HTTP thread must not touch Bukkit). */
    private <T> T sync(java.util.concurrent.Callable<T> c) throws Exception {
        return Bukkit.getScheduler().callSyncMethod(plugin, c).get();
    }

    private String status(String body) throws Exception {
        return sync(() -> {
            double[] tps = Bukkit.getTPS();
            StringBuilder players = new StringBuilder("[");
            boolean first = true;
            for (Player p : Bukkit.getOnlinePlayers()) {
                if (!first) players.append(',');
                players.append(Json.str(p.getName()));
                first = false;
            }
            players.append(']');
            return "{\"players\":" + Bukkit.getOnlinePlayers().size()
                    + ",\"names\":" + players
                    + ",\"tps\":" + String.format(java.util.Locale.ROOT, "%.2f", tps[0])
                    + ",\"mspt\":" + String.format(java.util.Locale.ROOT, "%.2f", Bukkit.getAverageTickTime())
                    + ",\"version\":" + Json.str(Bukkit.getMinecraftVersion())
                    + ",\"bridge\":" + Json.str(plugin.getPluginMeta().getVersion()) + "}";
        });
    }

    private String broadcast(String body) throws Exception {
        String msg = Json.field(body, "message");
        sync(() -> {
            Bukkit.broadcast(prefix().append(Component.text(msg, NamedTextColor.WHITE)));
            return null;
        });
        return "{\"ok\":true}";
    }

    /** A restart countdown: title + action bar + chat, so nobody misses it. */
    private String countdown(String body) throws Exception {
        int seconds = Integer.parseInt(Json.field(body, "seconds"));
        String reason = Json.field(body, "reason");
        sync(() -> {
            Component line = prefix().append(Component.text("restarting in " + seconds + "s", NamedTextColor.YELLOW));
            if (!reason.isBlank()) {
                line = line.append(Component.text(" — " + reason, NamedTextColor.GRAY));
            }
            Bukkit.broadcast(line);
            Title title = Title.title(
                    Component.text("Restart in " + seconds + "s", NamedTextColor.GOLD),
                    Component.text(reason.isBlank() ? "plugin updates" : reason, NamedTextColor.GRAY),
                    Title.Times.times(Duration.ZERO, Duration.ofSeconds(Math.min(seconds, 3)), Duration.ofMillis(500)));
            for (Player p : Bukkit.getOnlinePlayers()) {
                p.showTitle(title);
                p.sendActionBar(Component.text("Server restart in " + seconds + "s", NamedTextColor.YELLOW));
            }
            return null;
        });
        return "{\"ok\":true}";
    }

    private String notifyOps(String body) throws Exception {
        String msg = Json.field(body, "message");
        sync(() -> {
            Component c = prefix().append(Component.text(msg, NamedTextColor.AQUA));
            for (Player p : Bukkit.getOnlinePlayers()) {
                if (p.hasPermission("mcplug.admin")) {
                    p.sendMessage(c);
                }
            }
            Bukkit.getConsoleSender().sendMessage(c);
            return null;
        });
        return "{\"ok\":true}";
    }

    static Component prefix() {
        return Component.text("[mcplug] ", NamedTextColor.GREEN);
    }
}
