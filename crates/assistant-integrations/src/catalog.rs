#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    Codex,
    Devin,
    Windsurf,
    Gemini,
    Antigravity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientInfo {
    pub client: Client,
    pub slug: &'static str,
    pub display_name: &'static str,
    pub package_kind: &'static str,
}

pub const CLIENTS: [ClientInfo; 7] = [
    ClientInfo {
        client: Client::ClaudeCode,
        slug: "claude-code",
        display_name: "Claude Code",
        package_kind: "Claude Code plugin",
    },
    ClientInfo {
        client: Client::ClaudeDesktop,
        slug: "claude-desktop",
        display_name: "Claude Desktop",
        package_kind: "MCP Bundle",
    },
    ClientInfo {
        client: Client::Cursor,
        slug: "cursor",
        display_name: "Cursor",
        package_kind: "Agent Plugin",
    },
    ClientInfo {
        client: Client::Codex,
        slug: "codex",
        display_name: "ChatGPT and Codex",
        package_kind: "Codex plugin",
    },
    ClientInfo {
        client: Client::Devin,
        slug: "devin",
        display_name: "Devin Desktop",
        package_kind: "MCP plugin",
    },
    ClientInfo {
        client: Client::Gemini,
        slug: "gemini",
        display_name: "Gemini CLI",
        package_kind: "Gemini extension",
    },
    ClientInfo {
        client: Client::Antigravity,
        slug: "antigravity",
        display_name: "Antigravity",
        package_kind: "Antigravity plugin",
    },
];

const WINDSURF_COMPAT: ClientInfo = ClientInfo {
    client: Client::Windsurf,
    slug: "windsurf",
    display_name: "Windsurf",
    package_kind: "legacy JSON integration",
};

impl Client {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude-code" => Some(Self::ClaudeCode),
            "claude-desktop" => Some(Self::ClaudeDesktop),
            "cursor" => Some(Self::Cursor),
            "codex" => Some(Self::Codex),
            "devin" => Some(Self::Devin),
            "windsurf" => Some(Self::Windsurf),
            "gemini" => Some(Self::Gemini),
            "antigravity" => Some(Self::Antigravity),
            _ => None,
        }
    }

    pub fn info(self) -> &'static ClientInfo {
        if self == Self::Windsurf {
            return &WINDSURF_COMPAT;
        }
        CLIENTS
            .iter()
            .find(|info| info.client == self)
            .expect("every Client has catalog metadata")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_catalog_keeps_the_windsurf_compatibility_alias() {
        assert_eq!(Client::parse("windsurf"), Some(Client::Windsurf));
        assert_eq!(Client::parse("devin"), Some(Client::Devin));
        assert_eq!(CLIENTS.len(), 7);
        assert_eq!(Client::Devin.info().display_name, "Devin Desktop");
    }
}
