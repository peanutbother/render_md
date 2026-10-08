---
title: 'tools'
site_title: 'Lab'
---

<header class="note-header">
<nav class="breadcrumbs"><ul><li><a href="/">Lab</a></li><li><a href="/">lab</a></li><li>tools</li></ul></nav>
<div class="inline-title">tools</div>
</header>

<table class="properties">
<tbody>
<tr><th>tags</th><td><a class="tag" href="/tags/cli/">#cli</a></td></tr>
</tbody>
</table>

<a class="tag" href="/tags/cli/">#cli</a> helpers


<div class="callout" data-callout="tip">
<div class="callout-title">dry run <a class="tag" href="/tags/tip/">#tip</a></div>
<div class="callout-content">

Add `--check` to see what *would* change.

</div>
</div>



<div class="callout" data-callout="fix">
<div class="callout-title"><a class="tag" href="/tags/fix/">#fix</a></div>
<div class="callout-content">

> The job failed once a week.

Fixed by retrying.

</div>
</div>


| option | value |
| --- | --- |
| [Login](</lab/servers/auth/#login-sso--proxy>) | none |
| [web](</lab/servers/web/>) | yes |

```sh
if [[ $UPDATES -gt 0 ]]; then
  docker ps --format '\{{.Names}}'
fi
```

Inline code stays too: `[[ -f x ]]` and `\{{.Names}}`.

A block reference: [the login flow](</lab/servers/auth/#^login-flow>), and a broken one: <span class="broken-link" title="Not found: does not exist">does not exist</span>.

<section class="backlinks">
<div class="backlinks-title">Linked mentions</div>
<ul>
<li><a href="/">lab</a> <span class="backlink-folder">lab</span></li>
</ul>
</section>
