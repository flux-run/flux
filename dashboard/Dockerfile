# ─── Stage 1: Build Vite app ────────────────────────────────────────────────
FROM node:20-alpine AS builder

WORKDIR /app

# Install dependencies first (cached layer)
COPY package.json package-lock.json* ./
RUN npm ci

# Receive VITE_* variables as build args so they are baked into the bundle.
# Override with: docker build --build-arg VITE_API_URL=... .
ARG VITE_API_URL=http://localhost:8080
ARG VITE_GATEWAY_URL=http://localhost:8081

ENV VITE_API_URL=$VITE_API_URL
ENV VITE_GATEWAY_URL=$VITE_GATEWAY_URL

COPY . .
RUN npm run build

# ─── Stage 2: Serve with Nginx ───────────────────────────────────────────────
FROM nginx:1.25-alpine

# Routing config — docs/examples served statically; /dashboard → React SPA
COPY nginx.dev.conf /etc/nginx/conf.d/default.conf

# React SPA — served at /dashboard/* and /login
COPY --from=builder /app/dist /usr/share/nginx/html

# Static marketing site (landing, docs, examples, install script).
# These are copied from public/ into the image.
# Run `npm run prebuild` (or `node scripts/copy-static.js`) from the dashboard/
# directory before building this image to populate public/{docs,examples,assets,...}
COPY public/index.html  /usr/share/nginx/static/index.html
COPY public/install.sh  /usr/share/nginx/static/install.sh
COPY public/assets      /usr/share/nginx/static/assets
COPY public/docs        /usr/share/nginx/static/docs
COPY public/examples    /usr/share/nginx/static/examples

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
