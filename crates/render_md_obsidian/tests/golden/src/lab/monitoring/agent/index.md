---
title: 'agent'
site_title: 'Lab'
---

<header class="note-header">
<nav class="breadcrumbs"><ul><li><a href="/">Lab</a></li><li><a href="/">lab</a></li><li><a href="/lab/monitoring/">monitoring</a></li><li>agent</li></ul></nav>
<div class="inline-title">agent</div>
</header>

The agent is rolled out by a playbook.

## Add a new host {#add-a-new-host}

<details class="callout" data-callout="example" open>
<summary class="callout-title">Checklist</summary>
<div class="callout-content">

1. **DNS name**: add the host.
   `dig +short host.example.com`
2. **Roll out**:
   ```yaml
   agent_hosts: ['web']
   ```
3. **Check** it.

Hosts with docker get the [docker plugin](</lab/monitoring/docker-plugin/>) as well.

</div>
</details>


## Manual install {#manual-install}


<details class="callout" data-callout="note">
<summary class="callout-title">Debian / Ubuntu</summary>
<div class="callout-content">


<details class="embed">
<summary class="embed-title"><a href="/lab/monitoring/agent/debian/">Debian</a></summary>
<div class="embed-content">

```sh
apt install ./agent.deb
```

</div>
</details>


</div>
</details>

<section class="backlinks">
<div class="backlinks-title">Linked mentions</div>
<ul>
<li><a href="/lab/monitoring/telemetry-agent/">Telemetry agent</a> <span class="backlink-folder">lab/monitoring/Telemetry agent</span></li>
</ul>
</section>
