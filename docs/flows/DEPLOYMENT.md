# Deployment

Codoxear is a single-process server intended to run on the same machine as Codex CLI. In this deployment, the server is exposed to the public internet (direct port forward or reverse proxy). It does not provide TLS.

## Production environment (this host)
In this project, "production" refers to the service running on this host (the same machine that runs Codex CLI), not a separate environment.

## Minimal deployment (public access)
1. Set `CODEX_WEB_PASSWORD` in `.env` (loaded from the server working directory) or the environment.
2. Run `scripts/codoxear-server-dev` on the host machine (standard startup).
3. Start Codex sessions with `codoxear-broker -- <codex args>`.
4. Expose port `8743` to the internet (or proxy it) and visit `http://<public-host>:8743/` (or your HTTPS proxy URL).

## Network security
- The server does not provide TLS or strong authentication beyond a password.
- For public access, prefer a reverse proxy with TLS termination and IP allowlists if possible.
- If you terminate TLS in a proxy, set `CODEX_WEB_COOKIE_SECURE=1` or forward `X-Forwarded-Proto: https`.
- Treat the password as the only gate; assume anyone who can reach the port can observe or modify traffic.

## Public access options
- Direct port forward: expose `8743` on your router or cloud firewall, and restrict source IPs if possible.
- Reverse proxy: terminate TLS and optionally mount under a subpath via `CODEX_WEB_URL_PREFIX`.

## URL prefix
Use `CODEX_WEB_URL_PREFIX=/codoxear` to serve the UI and API under a subpath. Cookie scope follows the prefix.

## Runtime paths
Runtime state is stored under `~/.local/share/codoxear`:
- `socks/` broker sockets and metadata
- `hmac_secret` cookie signing key
- `uploads/` temporary uploaded images

## Fixed startup (standard)
Use `scripts/codoxear-server-dev` to keep the server working directory pinned to the repo root for consistent `.env` loading.

## This host (observed)
As of 2026-02-24, the running server process was started from `/root/code/codoxear`, so `.env` is read from `/root/code/codoxear/.env`.

## Env file location (this host)
- `/root/code/codoxear/.env` (because the server is running from `/root/code/codoxear`).

## Operational notes
- Terminal-owned sessions are attach-only and cannot be killed via the UI.
- Web-owned sessions can be deleted from the UI.
