package dev.antoinenz.mcplug.bridge;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import org.bukkit.Bukkit;
import org.bukkit.command.Command;
import org.bukkit.command.CommandExecutor;
import org.bukkit.command.CommandSender;
import org.jetbrains.annotations.NotNull;

/**
 * /mcplug status|check|update|restart — forwarded to the mcplug daemon on localhost with this
 * server's token. The daemon resolves the token to exactly this server, so an operator here can
 * never touch another instance. Permission mcplug.admin defaults to op.
 */
final class McplugCommand implements CommandExecutor {
    private final McplugBridge plugin;
    private final String token;
    private final String daemonUrl;
    private final HttpClient http = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(3)).build();

    McplugCommand(McplugBridge plugin, String token, String daemonUrl) {
        this.plugin = plugin;
        this.token = token;
        this.daemonUrl = daemonUrl.replaceAll("/+$", "");
    }

    @Override
    public boolean onCommand(@NotNull CommandSender sender, @NotNull Command cmd, @NotNull String label, String[] args) {
        if (!sender.hasPermission("mcplug.admin")) {
            sender.sendMessage(BridgeServer.prefix().append(Component.text("operators only", NamedTextColor.RED)));
            return true;
        }
        String action = args.length == 0 ? "status" : args[0].toLowerCase();
        String target = args.length > 1 ? String.join(" ", java.util.Arrays.copyOfRange(args, 1, args.length)) : "";
        if (!action.matches("status|check|update|restart|help")) {
            sender.sendMessage(BridgeServer.prefix().append(Component.text("usage: /mcplug status | check | update [plugin|all] | restart", NamedTextColor.GRAY)));
            return true;
        }
        if (action.equals("help")) {
            sender.sendMessage(BridgeServer.prefix().append(Component.text("status — what mcplug knows about this server\ncheck — look for plugin updates now\nupdate [plugin|all] — apply updates (restart when the server is empty)\nrestart — restart now with a countdown", NamedTextColor.GRAY)));
            return true;
        }
        sender.sendMessage(BridgeServer.prefix().append(Component.text("asking mcplug…", NamedTextColor.GRAY)));
        String body = "{\"action\":" + Json.str(action) + ",\"target\":" + Json.str(target) + ",\"by\":" + Json.str(sender.getName()) + "}";
        HttpRequest req = HttpRequest.newBuilder(URI.create(daemonUrl + "/v1/command"))
                .timeout(Duration.ofSeconds(120))
                .header("Authorization", "Bearer " + token)
                .header("Content-Type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(body))
                .build();
        http.sendAsync(req, HttpResponse.BodyHandlers.ofString()).whenComplete((resp, err) -> {
            String text;
            NamedTextColor color;
            if (err != null) {
                text = "mcplug daemon is not reachable (" + err.getClass().getSimpleName() + "). Is `mcplug daemon` running?";
                color = NamedTextColor.RED;
            } else if (resp.statusCode() != 200) {
                text = "mcplug: " + Json.field(resp.body(), "error");
                color = NamedTextColor.RED;
            } else {
                text = Json.field(resp.body(), "message");
                color = NamedTextColor.WHITE;
            }
            final String t = text;
            final NamedTextColor c = color;
            Bukkit.getScheduler().runTask(plugin, () -> {
                for (String line : t.split("\n")) {
                    sender.sendMessage(BridgeServer.prefix().append(Component.text(line, c)));
                }
            });
        });
        return true;
    }
}
