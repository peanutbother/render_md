---
blueprint: "[[server.blueprint]]"
service: nginx
node: 101
ip:
  - 192.0.2.10
  - 198.51.100.10
port:
  - "80"
  - "443"
domain: web.example.com
monitored: true
proxy_scheme: "https://"
owner: "[[auth]]"
tags:
  - docker
---
# nginx

Serves the static sites.

## Proxy config

| option | value |
| --- | --- |
| Scheme | https:// |
| [[auth#Login (sso / proxy)\|Auth Request]] | none |

## Monitoring

![[Telemetry agent]]
