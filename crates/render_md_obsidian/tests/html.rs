//! End-to-end: the fixture vault rendered to HTML (without Tailwind).

mod common;

use render_md_obsidian::{Severity, build};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

fn page(out: &Path, route: &str) -> String {
    fs::read_to_string(out.join(route.trim_matches('/')).join("index.html"))
        .unwrap_or_else(|e| panic!("{route}: {e}"))
}

#[test]
fn test_fixture_vault_renders() {
    let out = tempfile::tempdir().unwrap();
    let site = common::site_options(common::fixture_vault(), Some("lab/lab.md"));
    let report = build(common::build_options(site, out.path(), None)).unwrap();
    let out = out.path();

    assert!(
        report.render.failures.is_empty(),
        "{:?}",
        report.render.failures
    );
    let broken: Vec<String> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Broken)
        .map(|d| format!("{}: {}", d.source, d.message))
        .collect();
    assert_eq!(
        broken,
        [
            "lab/tools.md: unresolved link [[does not exist]]",
            "notes/misc.md: [[web#No such heading]]: 'No such heading' is not a heading or block in 'lab/servers/web.md'",
            "notes/misc.md: unresolved embed ![[missing note]]",
        ]
    );

    // No Obsidian syntax survives outside code, on any page.
    for entry in WalkDir::new(out).into_iter().filter_map(Result::ok) {
        if entry.file_name() != "index.html" {
            continue;
        }
        let prose = common::prose(&fs::read_to_string(entry.path()).unwrap());
        for marker in ["[[", "[!", "%%", "{{", "\u{E000}"] {
            assert!(
                !prose.contains(marker),
                "{marker} in {}",
                entry.path().display()
            );
        }
    }

    let home = page(out, "/");
    assert!(home.contains("<title>Lab overview</title>"), "{home}");
    assert!(home.contains("<div class=\"inline-title\">Lab overview</div>"));
    assert!(page(out, "/lab/tools/").contains("<title>tools · Lab</title>"));
    assert!(home.contains("<a href=\"#servers\">Servers</a>"), "{home}");
    assert!(home.contains("<h2 id=\"servers\">Servers</h2>"));
    assert!(home.contains("<th>name</th><th>service</th><th>node</th>"));
    // `inFolder` is recursive: the nested folder note is a row too.
    for server in [
        "/lab/servers/auth/",
        "/lab/servers/files/",
        "/lab/servers/proxy/",
        "/lab/servers/web/",
    ] {
        assert!(
            home.contains(&format!("<td><a href=\"{server}\">")),
            "{server}"
        );
    }
    assert!(
        home.contains("server.blueprint"),
        "excluded link is plain text"
    );

    let tools = page(out, "/lab/tools/");
    assert!(tools.contains("<a href=\"/lab/servers/auth/#login-sso--proxy\">Login</a>"));
    assert!(tools.contains("<a href=\"/lab/servers/auth/#^login-flow\">the login flow</a>"));
    assert!(tools.contains("<span class=\"broken-link\""));
    assert!(tools.contains("if [[ $UPDATES -gt 0 ]]; then"));
    assert!(tools.contains("docker ps --format '{{.Names}}'"));
    assert!(tools.contains("<code>{{.Names}}</code>"));
    assert!(tools.contains("<div class=\"callout\" data-callout=\"tip\">"));
    assert!(tools.contains("<a class=\"tag\" href=\"/tags/fix/\">#fix</a>"));

    let auth = page(out, "/lab/servers/auth/");
    assert!(auth.contains("<a id=\"^login-flow\" class=\"block-id\"></a>"));
    assert!(auth.contains("<h2 id=\"notes-1\">Notes</h2>"));

    let web = page(out, "/lab/servers/web/");
    assert!(web.contains("<tr><th>ip</th><td>192.0.2.10, 198.51.100.10</td></tr>"));
    assert!(web.contains("<a href=\"https://web.example.com\">web.example.com</a>"));
    assert!(
        web.contains("<tr><th>owner</th><td><a href=\"/lab/servers/auth/\">auth</a></td></tr>")
    );
    assert!(!web.contains("proxy_scheme"), "hidden by --hide-property");
    assert!(!web.contains("<th>blueprint</th>"));
    assert!(web.contains("<details class=\"embed\">"));
    assert!(web.contains("<h1 id=\"embed-telemetry-agent-setup\">Setup</h1>"));

    let board = page(out, "/lab/project/board/");
    assert!(board.contains("<div class=\"kanban\">"));
    assert!(!board.contains("kanban:settings"));

    let project = page(out, "/lab/project/");
    assert!(project.contains("move <a href=\"/lab/servers/web/\">web</a> to the new host"));
    assert!(project.contains("The notes board"));
    assert!(project.contains("read about"));

    let router = page(out, "/lab/router/");
    assert!(router.contains("<details class=\"query\" data-lang=\"dataviewjs\">"));
    assert!(router.contains("Plain text after the query. Visible."));

    let files = page(out, "/lab/servers/files/");
    assert!(files.contains("/etc/samba/smb.conf</a></summary>"));
    assert!(files.contains("# a comment, not a heading"));

    let agent = page(out, "/lab/monitoring/agent/");
    assert!(agent.contains("<pre><code class=\"language-yaml\">agent_hosts: ['web']"));
    assert!(
        agent.contains("apt install ./agent.deb"),
        "embed inside a callout"
    );

    // Backlinks, generated pages, redirects, assets.
    assert!(auth.contains("Linked mentions"));
    assert!(auth.contains("<a href=\"/lab/tools/\">tools</a>"));
    assert!(page(out, "/unlisted/").contains("<a href=\"/unlisted/a-note/\">a note</a>"));
    assert!(page(out, "/tags/docker/").contains("<a href=\"/lab/servers/web/\">web</a>"));
    assert!(page(out, "/lab/").contains("http-equiv=\"refresh\""));
    assert!(out.join("assets/logo.svg").is_file());
}
