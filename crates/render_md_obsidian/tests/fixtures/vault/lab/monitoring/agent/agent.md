The agent is rolled out by a playbook.

## Add a new host
> [!example]+ Checklist
> 1. **DNS name**: add the host.
>    `dig +short host.example.com`
> 2. **Roll out**:
>    ```yaml
>    agent_hosts: ['web']
>    ```
> 3. **Check** it.
>
> Hosts with docker get the [[docker plugin]] as well.

## Manual install

> [!note]- Debian / Ubuntu
> ![[Debian]]
