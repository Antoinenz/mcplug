package dev.antoinenz.mcplug.bridge;

/** Just enough JSON for flat objects with string/number fields; avoids pulling in a library. */
final class Json {
    private Json() {}

    static String str(String s) {
        if (s == null) return "null";
        StringBuilder b = new StringBuilder("\"");
        for (char c : s.toCharArray()) {
            switch (c) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                default -> {
                    if (c < 0x20) b.append(String.format("\\u%04x", (int) c));
                    else b.append(c);
                }
            }
        }
        return b.append('"').toString();
    }

    /** Value of a top-level field as a string ("" if missing). Handles quoted strings and bare numbers. */
    static String field(String json, String name) {
        String key = "\"" + name + "\"";
        int i = json.indexOf(key);
        if (i < 0) return "";
        i = json.indexOf(':', i + key.length());
        if (i < 0) return "";
        i++;
        while (i < json.length() && Character.isWhitespace(json.charAt(i))) i++;
        if (i >= json.length()) return "";
        if (json.charAt(i) == '"') {
            StringBuilder b = new StringBuilder();
            i++;
            while (i < json.length()) {
                char c = json.charAt(i++);
                if (c == '"') break;
                if (c == '\\' && i < json.length()) {
                    char n = json.charAt(i++);
                    switch (n) {
                        case 'n' -> b.append('\n');
                        case 't' -> b.append('\t');
                        case 'r' -> b.append('\r');
                        case 'u' -> {
                            b.append((char) Integer.parseInt(json.substring(i, i + 4), 16));
                            i += 4;
                        }
                        default -> b.append(n);
                    }
                } else {
                    b.append(c);
                }
            }
            return b.toString();
        }
        int j = i;
        while (j < json.length() && ",}]".indexOf(json.charAt(j)) < 0) j++;
        return json.substring(i, j).trim();
    }
}
