//! Tokenise a server start command to find the java binary and the server jar.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCommand {
    pub tokens: Vec<String>,
}

impl StartCommand {
    pub fn parse(cmd: &str) -> Self {
        Self { tokens: shell_split(cmd) }
    }

    /// First token, e.g. `java` or `/usr/lib/jvm/java-25-openjdk-amd64/bin/java`.
    pub fn java(&self) -> Option<&str> {
        self.tokens.first().map(String::as_str)
    }

    /// The argument after `-jar`.
    pub fn jar(&self) -> Option<&str> {
        let i = self.tokens.iter().position(|t| t == "-jar")?;
        self.tokens.get(i + 1).map(String::as_str)
    }

    /// The command with the jar argument replaced, quoting preserved where needed.
    pub fn with_jar(&self, jar: &str) -> String {
        let mut out = self.tokens.clone();
        if let Some(i) = out.iter().position(|t| t == "-jar") {
            if i + 1 < out.len() {
                out[i + 1] = jar.to_string();
            }
        }
        out.iter().map(|t| quote(t)).collect::<Vec<_>>().join(" ")
    }

    /// The command with the java binary replaced.
    pub fn with_java(&self, java: &str) -> String {
        let mut out = self.tokens.clone();
        if let Some(first) = out.first_mut() {
            *first = java.to_string();
        }
        out.iter().map(|t| quote(t)).collect::<Vec<_>>().join(" ")
    }
}

fn quote(t: &str) -> String {
    if t.is_empty() || t.chars().any(|c| c.is_whitespace() || c == '"' || c == '\'') {
        format!("\"{}\"", t.replace('"', "\\\""))
    } else {
        t.to_string()
    }
}

/// Minimal POSIX-ish splitter: whitespace separated, single/double quotes, backslash escapes.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_tok = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            Some(_) => cur.push(c),
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    in_tok = true;
                }
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                        in_tok = true;
                    }
                }
                c if c.is_whitespace() => {
                    if in_tok {
                        out.push(std::mem::take(&mut cur));
                        in_tok = false;
                    }
                }
                c => {
                    cur.push(c);
                    in_tok = true;
                }
            },
        }
    }
    if in_tok {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_java_and_jar() {
        let c = StartCommand::parse(
            "/usr/lib/jvm/java-25-openjdk-amd64/bin/java -Xms4096M -Xmx4096M -XX:+UseG1GC -jar paper-26.2-123.jar nogui",
        );
        assert_eq!(c.java(), Some("/usr/lib/jvm/java-25-openjdk-amd64/bin/java"));
        assert_eq!(c.jar(), Some("paper-26.2-123.jar"));
        assert!(c.with_jar("paper-26.2-130.jar").contains("-jar paper-26.2-130.jar nogui"));
    }

    #[test]
    fn handles_quotes() {
        let c = StartCommand::parse(r#"java -jar "my server.jar" --nogui"#);
        assert_eq!(c.jar(), Some("my server.jar"));
        assert_eq!(c.with_jar("x y.jar"), r#"java -jar "x y.jar" --nogui"#);
        assert_eq!(StartCommand::parse("bash start.sh").jar(), None);
    }
}
