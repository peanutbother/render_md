---
title: 'docker plugin'
site_title: 'Lab'
---

<header class="note-header">
<nav class="breadcrumbs"><ul><li><a href="/">Lab</a></li><li><a href="/">lab</a></li><li><a href="/lab/monitoring/">monitoring</a></li><li>docker plugin</li></ul></nav>
<div class="inline-title">docker plugin</div>
</header>

<a class="tag" href="/tags/docker/">#docker</a>

The plugin is configured in [Config](<#config>).

## Config {#config}

```ini
[DOCKER]
skip_sections: container_agent
```

## Known issues {#known-issues}


<details class="embed">
<summary class="embed-title"><a href="/lab/monitoring/docker-plugin/fix-restart-loop/">fix restart loop</a></summary>
<div class="embed-content">

<div class="callout" data-callout="fix">
<div class="callout-title"><a class="tag" href="/tags/fix/">#fix</a></div>
<div class="callout-content">

> `Docker node info` goes UNKNOWN while a container restarts.

Skip the section:
```ini
skip_sections: container_agent
```

</div>
</div>

</div>
</details>

<section class="backlinks">
<div class="backlinks-title">Linked mentions</div>
<ul>
<li><a href="/lab/monitoring/agent/">agent</a> <span class="backlink-folder">lab/monitoring/agent</span></li>
</ul>
</section>
