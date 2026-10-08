---
title: 'web'
site_title: 'Lab'
---

<header class="note-header">
<nav class="breadcrumbs"><ul><li><a href="/">Lab</a></li><li><a href="/">lab</a></li><li><a href="/lab/servers/">servers</a></li><li>web</li></ul></nav>
<div class="inline-title">web</div>
</header>

<table class="properties">
<tbody>
<tr><th>service</th><td>nginx</td></tr>
<tr><th>node</th><td>101</td></tr>
<tr><th>ip</th><td>192.0.2.10, 198.51.100.10</td></tr>
<tr><th>port</th><td>80, 443</td></tr>
<tr><th>domain</th><td><a href="https://web.example.com">web.example.com</a></td></tr>
<tr><th>monitored</th><td><span class="prop-true" title="true">✓</span></td></tr>
<tr><th>owner</th><td><a href="/lab/servers/auth/">auth</a></td></tr>
<tr><th>tags</th><td><a class="tag" href="/tags/docker/">#docker</a></td></tr>
</tbody>
</table>

# nginx {#nginx}

Serves the static sites.

## Proxy config {#proxy-config}

| option | value |
| --- | --- |
| Scheme | https:// |
| [Auth Request](</lab/servers/auth/#login-sso--proxy>) | none |

## Monitoring {#monitoring}


<details class="embed">
<summary class="embed-title"><a href="/lab/monitoring/telemetry-agent/">Telemetry agent</a></summary>
<div class="embed-content">

# Setup {#embed-telemetry-agent-setup}

The telemetry agent runs on every node, see [agent \> Add a new host](</lab/monitoring/agent/#add-a-new-host>).


<div class="callout" data-callout="warning">
<div class="callout-title">restarts</div>
<div class="callout-content">

Logs are replayed after a restart.

</div>
</div>


# Health {#embed-telemetry-agent-health}

The agent reports to [web](</lab/servers/web/>).

</div>
</details>

<section class="backlinks">
<div class="backlinks-title">Linked mentions</div>
<ul>
<li><a href="/lab/project/board/">Board</a> <span class="backlink-folder">lab/Project</span></li>
<li><a href="/lab/monitoring/telemetry-agent/">Telemetry agent</a> <span class="backlink-folder">lab/monitoring/Telemetry agent</span></li>
<li><a href="/lab/tools/">tools</a> <span class="backlink-folder">lab</span></li>
<li><a href="/notes/misc/">misc</a> <span class="backlink-folder">notes</span></li>
</ul>
</section>
