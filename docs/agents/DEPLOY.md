# Deploy

Nitro picks the target at build time, so one codebase ships to Node, Vercel, or Cloudflare Workers with the same security headers. On every target, set `VITE_APP_URL` to the public origin first. The canonical and social tags use it.

## Node

```bash
npm run build
PORT=8080 npm run start
```

The server listens on `PORT`, default `3000`, and on `HOST`. It serves prebuilt Brotli and gzip copies of assets but sends rendered HTML uncompressed, so put a compressing proxy or CDN in front of it, such as Caddy, nginx, or Cloudflare.

## Vercel

Import the repository in Vercel and add `VITE_APP_URL` under **Environment Variables**. Nitro detects Vercel during the build, so nothing else needs configuring.

## Cloudflare Workers

```bash
NITRO_PRESET=cloudflare_module npm run build
npx wrangler dev
npx wrangler login
npx wrangler deploy
```

`npx wrangler dev` runs the build in Cloudflare's runtime before you deploy. To deploy on every push instead, connect the repository with Workers Builds, with the same build command and `npx wrangler deploy` as the deploy command.

Workers run `workerd` with Node compatibility, not Node. There is no writable file system, and some Node APIs are missing. If server code uses a Node API, test it with `npx wrangler dev` first.
