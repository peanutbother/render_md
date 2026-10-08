---
tags: [cli]
---
#cli helpers

> [!tip] dry run #tip
> Add `--check` to see what *would* change.

> [!fix] #fix
> > The job failed once a week.
>
> Fixed by retrying.

| option | value |
| --- | --- |
| [[auth#Login (sso / proxy)\|Login]] | none |
| [[web]] | yes |

```sh
if [[ $UPDATES -gt 0 ]]; then
  docker ps --format '{{.Names}}'
fi
```

Inline code stays too: `[[ -f x ]]` and `{{.Names}}`.

A block reference: [[auth#^login-flow|the login flow]], and a broken one: [[does not exist]].
