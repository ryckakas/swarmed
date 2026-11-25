//! The user's own browser servers, pointed at the task's Chrome (#384).
//!
//! Playwright MCP and Chrome DevTools MCP each launch a browser of their own unless told where
//! one runs. They are recognised by what they run, never by the name the user gave them, and
//! told the session's address as each takes it: `--cdp-endpoint` for Playwright MCP,
//! `--browser-url` for Chrome DevTools MCP. Nothing else about them changes.
//!
//! Left as they are: a server that already says where its browser is, one that drives Firefox
//! or `WebKit`, one that stands in for a device (Playwright MCP refuses that with a browser to
//! connect to), one that works through the browser extension, one reached over HTTP (it runs
//! apart, and cannot be given an argument), and every other kind of browser server.

use std::collections::BTreeMap;

use crate::mcp::{McpServer, McpTransport};

/// Which of the browser servers Swarmed knows a server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Browsing {
    /// Playwright MCP, `@playwright/mcp`.
    Playwright,
    /// Chrome DevTools MCP, `chrome-devtools-mcp`.
    DevTools,
}

/// The options of Chrome DevTools MCP that it refuses beside `--browser-url`, with a value
/// each: they only say how to launch a browser, which it no longer does.
const DEVTOOLS_LAUNCHING: [&str; 6] = [
    "--channel",
    "--executable-path",
    "--executablePath",
    "-e",
    "--user-data-dir",
    "--userDataDir",
];

/// The options that say where a Chrome DevTools MCP's browser already is.
const DEVTOOLS_POINTED: [&str; 7] = [
    "--browser-url",
    "--browserUrl",
    "-u",
    "--ws-endpoint",
    "--wsEndpoint",
    "-w",
    "--auto-connect",
];

/// The options that say where a Playwright MCP's browser is, or that it uses the extension.
const PLAYWRIGHT_POINTED: [&str; 3] = ["--cdp-endpoint", "--extension", "--endpoint"];

/// The variables that do the same for Playwright MCP.
const PLAYWRIGHT_POINTED_ENV: [&str; 2] =
    ["PLAYWRIGHT_MCP_CDP_ENDPOINT", "PLAYWRIGHT_MCP_EXTENSION"];

/// The options that have Playwright MCP stand in for a device. It refuses to start with one
/// of them and a browser to connect to ("Device emulation is not supported with
/// cdpEndpoint"), so a server set up with one is left to launch its own.
const PLAYWRIGHT_DEVICE: [&str; 2] = ["--device", "--mobile"];

/// The variables that do the same.
const PLAYWRIGHT_DEVICE_ENV: [&str; 2] = ["PLAYWRIGHT_MCP_DEVICE", "PLAYWRIGHT_MCP_MOBILE"];

/// The variable that names Playwright MCP's browser, as `--browser` does.
const PLAYWRIGHT_BROWSER_ENV: &str = "PLAYWRIGHT_MCP_BROWSER";

/// Which browser server `server` is, by what it runs.
pub(crate) fn browsing(server: &McpServer) -> Option<Browsing> {
    let McpTransport::Stdio { command, args, .. } = &server.transport else {
        return None;
    };
    std::iter::once(command)
        .chain(args)
        .find_map(|word| package_of(word))
}

/// The package a word of a command names: a package as `npx` takes it, with or without its
/// version, or a path to its program or into its folder.
fn package_of(word: &str) -> Option<Browsing> {
    let bare = without_version(word);
    let name = bare.rsplit('/').next().unwrap_or(bare);
    let name = name.strip_suffix(".js").unwrap_or(name);
    let playwright = bare == "@playwright/mcp"
        || bare.contains("/@playwright/mcp/")
        || bare.ends_with("/@playwright/mcp")
        || ["playwright-mcp", "mcp-server-playwright"].contains(&name);
    let devtools = name == "chrome-devtools-mcp" || bare.contains("/chrome-devtools-mcp/");
    if playwright {
        Some(Browsing::Playwright)
    } else if devtools {
        Some(Browsing::DevTools)
    } else {
        None
    }
}

/// A package's name without the `@version` after it: `@playwright/mcp@latest` is
/// `@playwright/mcp`, `chrome-devtools-mcp@0.12` is `chrome-devtools-mcp`.
fn without_version(word: &str) -> &str {
    let from = usize::from(word.starts_with('@'));
    match word[from..].rfind('@') {
        Some(at) if !word[from..from + at].ends_with('/') => &word[..from + at],
        _ => word,
    }
}

/// `server` pointed at `endpoint`, the session's address of the task's Chrome; `None` if it is
/// no browser server Swarmed knows, or one left as it is.
pub(crate) fn pointed(server: &McpServer, endpoint: &str) -> Option<McpServer> {
    let McpTransport::Stdio { command, args, env } = &server.transport else {
        return None;
    };
    let args = match browsing(server)? {
        Browsing::Playwright => playwright_pointed(args, env, endpoint)?,
        Browsing::DevTools => devtools_pointed(args, endpoint)?,
    };
    Some(McpServer {
        name: server.name.clone(),
        transport: McpTransport::Stdio {
            command: command.clone(),
            args,
            env: env.clone(),
        },
    })
}

fn playwright_pointed(
    args: &[String],
    env: &BTreeMap<String, String>,
    endpoint: &str,
) -> Option<Vec<String>> {
    let told = |options: &[&str], names: &[&str]| {
        args.iter().any(|arg| names_one_of(arg, options))
            || names.iter().any(|name| env.contains_key(*name))
    };
    let pointed = told(&PLAYWRIGHT_POINTED, &PLAYWRIGHT_POINTED_ENV);
    let device = told(&PLAYWRIGHT_DEVICE, &PLAYWRIGHT_DEVICE_ENV);
    if pointed || device || other_engine(args, env) {
        return None;
    }
    let mut args = args.to_vec();
    args.extend(["--cdp-endpoint".to_string(), endpoint.to_string()]);
    Some(args)
}

/// Whether Playwright MCP is told to drive Firefox or `WebKit`, which no Chrome stands in for:
/// by its option, else by its variable.
fn other_engine(args: &[String], env: &BTreeMap<String, String>) -> bool {
    let engine = args.iter().enumerate().find_map(|(at, arg)| {
        let value = arg.strip_prefix("--browser=");
        let next = (arg == "--browser").then(|| args.get(at + 1)).flatten();
        value.or_else(|| next.map(String::as_str))
    });
    let engine = engine.or_else(|| env.get(PLAYWRIGHT_BROWSER_ENV).map(String::as_str));
    engine.is_some_and(|engine| matches!(engine, "firefox" | "webkit"))
}

fn devtools_pointed(args: &[String], endpoint: &str) -> Option<Vec<String>> {
    if args.iter().any(|arg| names_one_of(arg, &DEVTOOLS_POINTED)) {
        return None;
    }
    let mut kept = Vec::with_capacity(args.len() + 1);
    let mut words = args.iter();
    while let Some(arg) = words.next() {
        if DEVTOOLS_LAUNCHING.contains(&arg.as_str()) {
            // Its value goes with it.
            words.next();
        } else if !names_one_of(arg, &DEVTOOLS_LAUNCHING) {
            kept.push(arg.clone());
        }
    }
    kept.push(format!("--browser-url={endpoint}"));
    Some(kept)
}

/// Whether `arg` is one of `options`, alone or as `--option=value`.
fn names_one_of(arg: &str, options: &[&str]) -> bool {
    let name = arg.split_once('=').map_or(arg, |(name, _)| name);
    options.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "http://127.0.0.1:50123";

    fn stdio(command: &str, args: &[&str]) -> McpServer {
        McpServer {
            name: "browser".into(),
            transport: McpTransport::Stdio {
                command: command.into(),
                args: args.iter().map(ToString::to_string).collect(),
                env: BTreeMap::new(),
            },
        }
    }

    fn with_variable(mut server: McpServer, name: &str, value: &str) -> McpServer {
        if let McpTransport::Stdio { env, .. } = &mut server.transport {
            env.insert(name.into(), value.into());
        }
        server
    }

    fn args_of(server: Option<McpServer>) -> Option<Vec<String>> {
        match server?.transport {
            McpTransport::Stdio { args, .. } => Some(args),
            McpTransport::Http { .. } => None,
        }
    }

    #[test]
    fn the_servers_are_known_by_what_they_run_however_they_are_started() {
        let playwright = [
            stdio("npx", &["@playwright/mcp@latest"]),
            stdio("npx", &["-y", "@playwright/mcp"]),
            stdio("pnpm", &["dlx", "@playwright/mcp@0.0.85", "--headless"]),
            stdio("/home/me/tools/node_modules/.bin/playwright-mcp", &[]),
            stdio("mcp-server-playwright", &[]),
            stdio("node", &["/opt/mcp/node_modules/@playwright/mcp/cli.js"]),
        ];
        for server in &playwright {
            assert_eq!(browsing(server), Some(Browsing::Playwright), "{server:?}");
        }
        let devtools = [
            stdio("npx", &["-y", "chrome-devtools-mcp@latest"]),
            stdio(
                "/home/me/.local/share/chrome-devtools-mcp/node_modules/.bin/chrome-devtools-mcp",
                &["--isolated"],
            ),
            stdio("node", &["/opt/chrome-devtools-mcp/build/src/index.js"]),
        ];
        for server in &devtools {
            assert_eq!(browsing(server), Some(Browsing::DevTools), "{server:?}");
        }
        // Named like one, running something else: the name the user gave it says nothing.
        let others = [
            stdio("npx", &["@modelcontextprotocol/server-puppeteer"]),
            stdio("npx", &["playwright", "test"]),
            stdio("uvx", &["mcp-server-fetch"]),
        ];
        for server in &others {
            assert_eq!(browsing(server), None, "{server:?}");
        }
        let http = McpServer {
            name: "playwright".into(),
            transport: McpTransport::Http {
                url: "http://localhost:8931/mcp".into(),
                headers: BTreeMap::new(),
            },
        };
        assert_eq!(browsing(&http), None);
    }

    #[test]
    fn playwright_mcp_is_given_the_address_as_its_cdp_endpoint() {
        let pointed = pointed(&stdio("npx", &["@playwright/mcp@latest", "--headless"]), AT);
        assert_eq!(
            args_of(pointed),
            Some(vec![
                "@playwright/mcp@latest".into(),
                "--headless".into(),
                "--cdp-endpoint".into(),
                AT.into(),
            ])
        );
    }

    #[test]
    fn chrome_devtools_mcp_is_given_the_address_and_loses_what_only_launching_needs() {
        let server = stdio(
            "npx",
            &[
                "chrome-devtools-mcp@latest",
                "--channel",
                "canary",
                "--isolated",
                "--user-data-dir=/tmp/x",
                "-e",
                "/Applications/Chrome.app",
                "--slim",
            ],
        );
        assert_eq!(
            args_of(pointed(&server, AT)),
            Some(vec![
                "chrome-devtools-mcp@latest".into(),
                "--isolated".into(),
                "--slim".into(),
                format!("--browser-url={AT}"),
            ])
        );
    }

    #[test]
    fn a_server_that_says_where_its_browser_is_or_drives_another_engine_is_left_as_it_is() {
        let left = [
            stdio(
                "npx",
                &["@playwright/mcp", "--cdp-endpoint", "http://127.0.0.1:9222"],
            ),
            stdio("npx", &["@playwright/mcp", "--extension"]),
            stdio("npx", &["@playwright/mcp", "--browser", "firefox"]),
            stdio("npx", &["@playwright/mcp", "--browser=webkit"]),
            stdio("npx", &["@playwright/mcp", "--device", "iPhone 15"]),
            stdio("npx", &["@playwright/mcp", "--device=Pixel 5"]),
            stdio("npx", &["@playwright/mcp", "--mobile"]),
            stdio(
                "npx",
                &["chrome-devtools-mcp", "--browser-url=http://127.0.0.1:9222"],
            ),
            stdio(
                "npx",
                &["chrome-devtools-mcp", "-w", "ws://127.0.0.1:9222/x"],
            ),
            stdio("npx", &["chrome-devtools-mcp", "--auto-connect"]),
            stdio("npx", &["@modelcontextprotocol/server-puppeteer"]),
        ];
        for server in &left {
            assert_eq!(pointed(server, AT), None, "{server:?}");
        }
        // Chrome and Edge are Chromium: told where the task's Chrome is, they use it.
        let chrome = stdio("npx", &["@playwright/mcp", "--browser", "msedge"]);
        assert!(pointed(&chrome, AT).is_some());
        let by_env = with_variable(
            stdio("npx", &["@playwright/mcp"]),
            "PLAYWRIGHT_MCP_CDP_ENDPOINT",
            "http://127.0.0.1:9222",
        );
        assert_eq!(pointed(&by_env, AT), None);
        let left_by_env = [
            ("PLAYWRIGHT_MCP_DEVICE", "iPhone 15"),
            ("PLAYWRIGHT_MCP_MOBILE", "true"),
            ("PLAYWRIGHT_MCP_BROWSER", "firefox"),
        ];
        for (name, value) in left_by_env {
            let server = with_variable(stdio("npx", &["@playwright/mcp"]), name, value);
            assert_eq!(pointed(&server, AT), None, "{name}");
        }
    }

    #[test]
    fn a_version_after_a_package_is_left_out_and_a_scope_kept() {
        assert_eq!(without_version("@playwright/mcp@latest"), "@playwright/mcp");
        assert_eq!(without_version("@playwright/mcp"), "@playwright/mcp");
        assert_eq!(
            without_version("chrome-devtools-mcp@0.12.0"),
            "chrome-devtools-mcp"
        );
        assert_eq!(
            without_version("/lib/node_modules/@playwright/mcp/cli.js"),
            "/lib/node_modules/@playwright/mcp/cli.js"
        );
        assert_eq!(without_version("plain"), "plain");
    }
}
