package dev.antoinenz.mcplug.bridge;

import java.security.SecureRandom;
import java.util.Base64;
import org.bukkit.plugin.java.JavaPlugin;

public final class McplugBridge extends JavaPlugin {
    private BridgeServer server;

    @Override
    public void onEnable() {
        saveDefaultConfig();
        String token = getConfig().getString("token", "");
        if (token == null || token.isBlank()) {
            byte[] b = new byte[24];
            new SecureRandom().nextBytes(b);
            token = Base64.getUrlEncoder().withoutPadding().encodeToString(b);
            getConfig().set("token", token);
            saveConfig();
            getLogger().info("generated a bridge token in plugins/McplugBridge/config.yml");
        }
        int port = getConfig().getInt("port", 25580);
        try {
            server = new BridgeServer(this, port, token);
            server.start();
            getLogger().info("listening on 127.0.0.1:" + port);
        } catch (Exception e) {
            getLogger().severe("could not start the bridge listener on port " + port + ": " + e.getMessage());
        }
        getCommand("mcplug").setExecutor(new McplugCommand(this, token, getConfig().getString("daemon-url", "http://127.0.0.1:25581")));
    }

    @Override
    public void onDisable() {
        if (server != null) {
            server.stop();
        }
    }
}
