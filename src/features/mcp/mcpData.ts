export type McpCategory =
  | "All"
  | "Social"
  | "Developer"
  | "Productivity"
  | "Database"
  | "Search"
  | "Browser";

export type McpEnvKey = {
  key: string;
  label: string;
  placeholder?: string;
  secret?: boolean;
};

export type McpInstallConfig = {
  /** The executable to run, e.g. "npx" */
  command: string;
  /** Args passed to the command, e.g. ["-y", "@modelcontextprotocol/server-github"] */
  args: string[];
  /** Environment variables required by this server */
  envKeys: McpEnvKey[];
};

export type McpServer = {
  id: string;
  name: string;
  description: string;
  category: McpCategory;
  repo: string;
  /** Human-readable install snippet shown in the card */
  install?: string;
  icon: string;
  official?: boolean;
  stars?: string;
  /** If present, the server can be connected from within the app */
  installConfig?: McpInstallConfig;
};

export const MCP_CATEGORIES: McpCategory[] = [
  "All",
  "Social",
  "Developer",
  "Productivity",
  "Database",
  "Search",
  "Browser",
];

export const MCP_SERVERS: McpServer[] = [
  // ── Developer ────────────────────────────────────────────────────────────
  {
    id: "github",
    name: "GitHub",
    description: "Official GitHub MCP server — repos, PRs, issues, workflows, code search.",
    category: "Developer",
    repo: "https://github.com/github/github-mcp-server",
    install: "npx @modelcontextprotocol/server-github",
    icon: "🐙",
    official: true,
    stars: "8k+",
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-github"],
      envKeys: [
        {
          key: "GITHUB_PERSONAL_ACCESS_TOKEN",
          label: "GitHub Personal Access Token",
          placeholder: "ghp_...",
          secret: true,
        },
      ],
    },
  },
  {
    id: "filesystem",
    name: "Filesystem",
    description: "Secure file read/write with configurable directory access controls.",
    category: "Developer",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem",
    install: "npx @modelcontextprotocol/server-filesystem",
    icon: "📁",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-filesystem", "/"],
      envKeys: [],
    },
  },
  {
    id: "git",
    name: "Git",
    description: "Read, search, and manipulate Git repositories directly.",
    category: "Developer",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/git",
    install: "npx @modelcontextprotocol/server-git",
    icon: "🌿",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-git"],
      envKeys: [],
    },
  },
  {
    id: "fetch",
    name: "Fetch",
    description: "Fetch any URL and convert web content to markdown for LLM use.",
    category: "Developer",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/fetch",
    install: "npx @modelcontextprotocol/server-fetch",
    icon: "🌐",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-fetch"],
      envKeys: [],
    },
  },
  {
    id: "memory",
    name: "Memory",
    description: "Persistent memory system using a knowledge graph to store entities and relations.",
    category: "Developer",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/memory",
    install: "npx @modelcontextprotocol/server-memory",
    icon: "🧠",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-memory"],
      envKeys: [],
    },
  },
  {
    id: "sequential-thinking",
    name: "Sequential Thinking",
    description: "Dynamic problem-solving through structured thought sequences and branching.",
    category: "Developer",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking",
    install: "npx @modelcontextprotocol/server-sequential-thinking",
    icon: "🔗",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-sequential-thinking"],
      envKeys: [],
    },
  },

  // ── Browser ───────────────────────────────────────────────────────────────
  {
    id: "playwright",
    name: "Playwright",
    description: "Official Microsoft Playwright MCP — browser automation via accessibility snapshots.",
    category: "Browser",
    repo: "https://github.com/microsoft/playwright-mcp",
    install: "npx @playwright/mcp",
    icon: "🎭",
    official: true,
    stars: "12k+",
    installConfig: {
      command: "npx",
      args: ["-y", "@playwright/mcp"],
      envKeys: [],
    },
  },
  {
    id: "puppeteer",
    name: "Puppeteer",
    description: "Browser automation via Puppeteer — screenshots, JS execution, form interaction.",
    category: "Browser",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/puppeteer",
    install: "npx @modelcontextprotocol/server-puppeteer",
    icon: "🤖",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-puppeteer"],
      envKeys: [],
    },
  },

  // ── Social ────────────────────────────────────────────────────────────────
  {
    id: "instagram-mcpware",
    name: "Instagram",
    description: "23 tools for Instagram Graph API — posts, comments, DMs, stories, analytics.",
    category: "Social",
    repo: "https://github.com/mcpware/instagram-mcp",
    install: "npx @mcpware/instagram-mcp",
    icon: "📸",
    stars: "200+",
    installConfig: {
      command: "npx",
      args: ["-y", "@mcpware/instagram-mcp"],
      envKeys: [
        {
          key: "INSTAGRAM_ACCESS_TOKEN",
          label: "Instagram Access Token",
          placeholder: "IGQVJ...",
          secret: true,
        },
        {
          key: "INSTAGRAM_BUSINESS_ACCOUNT_ID",
          label: "Instagram Business Account ID",
          placeholder: "17841...",
        },
      ],
    },
  },
  {
    id: "instagram-ig-mcp",
    name: "Instagram Business",
    description: "Production-ready Instagram Business Graph API server for AI applications.",
    category: "Social",
    repo: "https://github.com/jlbadano/ig-mcp",
    icon: "📷",
  },
  {
    id: "twitter-x",
    name: "X / Twitter",
    description: "Post, search, read, and engage on X (Twitter) via the official API.",
    category: "Social",
    repo: "https://github.com/Infatoshi/x-mcp",
    icon: "𝕏",
    stars: "300+",
    installConfig: {
      command: "npx",
      args: ["-y", "x-mcp"],
      envKeys: [
        { key: "TWITTER_API_KEY", label: "API Key", secret: true },
        { key: "TWITTER_API_SECRET", label: "API Secret", secret: true },
        { key: "TWITTER_ACCESS_TOKEN", label: "Access Token", secret: true },
        { key: "TWITTER_ACCESS_SECRET", label: "Access Token Secret", secret: true },
      ],
    },
  },
  {
    id: "linkedin",
    name: "LinkedIn",
    description: "Access LinkedIn profiles, companies, job listings, and messages via browser automation.",
    category: "Social",
    repo: "https://github.com/stickerdaniel/linkedin-mcp-server",
    icon: "💼",
    stars: "150+",
  },
  {
    id: "linkedin-analytics",
    name: "LinkedIn Analytics",
    description: "AI-powered LinkedIn analytics, content creation, and engagement automation.",
    category: "Social",
    repo: "https://github.com/southleft/linkedin-mcp",
    icon: "📊",
  },
  {
    id: "facebook",
    name: "Facebook",
    description: "Automate Facebook posts, moderate comments, fetch insights and sentiment filtering.",
    category: "Social",
    repo: "https://github.com/HagaiHen/facebook-mcp-server",
    icon: "👤",
    stars: "100+",
    installConfig: {
      command: "npx",
      args: ["-y", "facebook-mcp-server"],
      envKeys: [
        {
          key: "FACEBOOK_ACCESS_TOKEN",
          label: "Facebook Page Access Token",
          placeholder: "EAABs...",
          secret: true,
        },
        {
          key: "FACEBOOK_PAGE_ID",
          label: "Facebook Page ID",
          placeholder: "123456789",
        },
      ],
    },
  },
  {
    id: "meta-ads",
    name: "Meta Ads",
    description: "Manage Facebook and Instagram Ads campaigns, budgets, and analytics.",
    category: "Social",
    repo: "https://github.com/pipeboard-co/meta-ads-mcp",
    icon: "📣",
    installConfig: {
      command: "npx",
      args: ["-y", "meta-ads-mcp"],
      envKeys: [
        {
          key: "META_ACCESS_TOKEN",
          label: "Meta Ads Access Token",
          secret: true,
        },
        {
          key: "META_AD_ACCOUNT_ID",
          label: "Ad Account ID",
          placeholder: "act_123...",
        },
      ],
    },
  },

  // ── Productivity ──────────────────────────────────────────────────────────
  {
    id: "slack",
    name: "Slack",
    description: "Post messages, read channel history, look up users, and manage Slack workspaces.",
    category: "Productivity",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/slack",
    install: "npx @modelcontextprotocol/server-slack",
    icon: "💬",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-slack"],
      envKeys: [
        {
          key: "SLACK_BOT_TOKEN",
          label: "Slack Bot Token",
          placeholder: "xoxb-...",
          secret: true,
        },
        {
          key: "SLACK_TEAM_ID",
          label: "Slack Team ID",
          placeholder: "T01234...",
        },
      ],
    },
  },
  {
    id: "notion",
    name: "Notion",
    description: "Query, create, and update Notion pages, databases, and blocks.",
    category: "Productivity",
    repo: "https://github.com/modelcontextprotocol/servers",
    icon: "📓",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-notion"],
      envKeys: [
        {
          key: "NOTION_API_KEY",
          label: "Notion Integration Token",
          placeholder: "secret_...",
          secret: true,
        },
      ],
    },
  },
  {
    id: "gdrive",
    name: "Google Drive",
    description: "Read/write Google Drive files, Docs, and Sheets with full Workspace integration.",
    category: "Productivity",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/gdrive",
    install: "npx @modelcontextprotocol/server-gdrive",
    icon: "☁️",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-gdrive"],
      envKeys: [],
    },
  },
  {
    id: "gmail",
    name: "Gmail",
    description: "Full Gmail integration with auto-authentication — read, send, search emails.",
    category: "Productivity",
    repo: "https://github.com/GongRzhe/Gmail-MCP-Server",
    icon: "✉️",
    stars: "400+",
    installConfig: {
      command: "npx",
      args: ["-y", "gmail-mcp-server"],
      envKeys: [
        {
          key: "GMAIL_CLIENT_ID",
          label: "OAuth Client ID",
          secret: true,
        },
        {
          key: "GMAIL_CLIENT_SECRET",
          label: "OAuth Client Secret",
          secret: true,
        },
        {
          key: "GMAIL_REFRESH_TOKEN",
          label: "OAuth Refresh Token",
          secret: true,
        },
      ],
    },
  },
  {
    id: "jira-atlassian",
    name: "Jira (Atlassian Official)",
    description: "Official Atlassian remote MCP server for Jira and Confluence.",
    category: "Productivity",
    repo: "https://github.com/atlassian/atlassian-mcp-server",
    icon: "🔷",
    official: true,
    stars: "500+",
    installConfig: {
      command: "npx",
      args: ["-y", "@atlassian/mcp-server"],
      envKeys: [
        {
          key: "ATLASSIAN_API_TOKEN",
          label: "Atlassian API Token",
          placeholder: "ATATT3...",
          secret: true,
        },
        {
          key: "ATLASSIAN_EMAIL",
          label: "Atlassian Account Email",
          placeholder: "you@company.com",
        },
        {
          key: "ATLASSIAN_BASE_URL",
          label: "Jira Base URL",
          placeholder: "https://yourorg.atlassian.net",
        },
      ],
    },
  },
  {
    id: "jira-community",
    name: "Jira Community",
    description: "Production-ready Jira Cloud MCP — issues, sprints, comments, transitions.",
    category: "Productivity",
    repo: "https://github.com/OrenGrinker/jira-mcp-server",
    icon: "🎯",
  },

  // ── Database ──────────────────────────────────────────────────────────────
  {
    id: "sqlite",
    name: "SQLite",
    description: "Query and inspect local SQLite databases with schema exploration.",
    category: "Database",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/sqlite",
    install: "npx @modelcontextprotocol/server-sqlite",
    icon: "🗃️",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-sqlite"],
      envKeys: [
        {
          key: "SQLITE_DB_PATH",
          label: "Database File Path",
          placeholder: "/path/to/database.db",
        },
      ],
    },
  },
  {
    id: "postgres",
    name: "PostgreSQL",
    description: "PostgreSQL integration — query, analyze performance, optimize indexes.",
    category: "Database",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/postgres",
    install: "npx @modelcontextprotocol/server-postgres",
    icon: "🐘",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-postgres"],
      envKeys: [
        {
          key: "POSTGRES_CONNECTION_STRING",
          label: "Connection String",
          placeholder: "postgresql://user:pass@localhost:5432/mydb",
          secret: true,
        },
      ],
    },
  },

  // ── Search ────────────────────────────────────────────────────────────────
  {
    id: "perplexity-official",
    name: "Perplexity AI",
    description: "Official Perplexity MCP — real-time web search with AI-powered reasoning.",
    category: "Search",
    repo: "https://github.com/perplexityai/modelcontextprotocol",
    icon: "🔎",
    official: true,
    stars: "600+",
    installConfig: {
      command: "npx",
      args: ["-y", "perplexity-mcp"],
      envKeys: [
        {
          key: "PERPLEXITY_API_KEY",
          label: "Perplexity API Key",
          placeholder: "pplx-...",
          secret: true,
        },
      ],
    },
  },
  {
    id: "brave-search",
    name: "Brave Search",
    description: "Web and local search using Brave Search API with privacy-first results.",
    category: "Search",
    repo: "https://github.com/modelcontextprotocol/servers/tree/main/src/brave-search",
    install: "npx @modelcontextprotocol/server-brave-search",
    icon: "🦁",
    official: true,
    installConfig: {
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-brave-search"],
      envKeys: [
        {
          key: "BRAVE_API_KEY",
          label: "Brave Search API Key",
          placeholder: "BSA...",
          secret: true,
        },
      ],
    },
  },
];
